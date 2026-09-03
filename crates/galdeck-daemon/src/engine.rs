//! The device engine: owns the HID handle on one thread, applies pages,
//! dispatches input events to actions, and services control requests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use galdeck_hid::{Buttons, Encoders, Event, Galleon, Rgb};
use galdeck_ipc::{Request, Response, Status};

use crate::config::{Config, EncoderConfig, KeyConfig, Page};
use crate::render;

const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_KEY_COLOR: Rgb = Rgb::new(24, 26, 32);
/// Cap on commands spawned for one coalesced rotation report.
const MAX_DETENTS_PER_EVENT: u32 = 8;

/// A control request paired with its reply channel.
pub struct ControlMsg {
    pub request: Request,
    pub reply: Sender<Response>,
}

pub struct Engine {
    config_path: PathBuf,
    config: Config,
    font: Option<galdeck_hid::Font>,
    page_index: usize,
    brightness: u8,
    device: Option<DeviceState>,
    last_connect_attempt: Option<Instant>,
    control_rx: Receiver<ControlMsg>,
    shutdown: Arc<AtomicBool>,
}

struct DeviceState {
    deck: Galleon,
    firmware: String,
    serial: String,
}

impl Engine {
    pub fn new(
        config_path: PathBuf,
        config: Config,
        control_rx: Receiver<ControlMsg>,
        shutdown: Arc<AtomicBool>,
    ) -> Result<Self> {
        let font = render::load_font(config.font.as_deref());
        let brightness = config.brightness;
        Ok(Engine {
            config_path,
            config,
            font,
            page_index: 0,
            brightness,
            device: None,
            last_connect_attempt: None,
            control_rx,
            shutdown,
        })
    }

    pub fn run(&mut self) {
        log::info!("engine started, config: {}", self.config_path.display());
        while !self.shutdown.load(Ordering::Relaxed) {
            self.service_control();

            if self.device.is_none() {
                self.maybe_connect();
                if self.device.is_none() {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }

            let state = self.device.as_mut().unwrap();
            match state.deck.poll(Duration::from_millis(200)) {
                Ok(events) => {
                    // A keepalive gap (suspend, long stall) means the module
                    // re-entered software mode and the firmware wiped our
                    // state — redraw the page.
                    if state.deck.take_mode_reentry() {
                        log::info!("module re-entered software mode, re-applying page");
                        self.apply_page();
                    }
                    for event in events {
                        self.handle_event(event);
                    }
                }
                Err(e) => {
                    log::warn!("device error, will reconnect: {e}");
                    self.device = None;
                }
            }
        }

        // Leave the module tidy: blank everything and hand it back to
        // hardware mode via the logo screen.
        if let Some(state) = self.device.as_mut() {
            let _ = state.deck.clear_all();
            let _ = state.deck.reset_to_logo();
        }
        log::info!("engine stopped");
    }

    fn maybe_connect(&mut self) {
        if let Some(last) = self.last_connect_attempt {
            if last.elapsed() < RECONNECT_INTERVAL {
                return;
            }
        }
        self.last_connect_attempt = Some(Instant::now());

        let api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(e) => {
                log::warn!("hidapi init failed: {e}");
                return;
            }
        };
        match Galleon::open(&api) {
            Ok(mut deck) => {
                let firmware = deck.firmware_version().unwrap_or_else(|_| "unknown".into());
                let serial = deck.serial_number().unwrap_or_else(|_| "unknown".into());
                log::info!("connected: firmware {firmware}, serial {serial}");
                if !galdeck_hid::ids::VALIDATED_FIRMWARES.contains(&firmware.as_str()) {
                    log::warn!(
                        "firmware {firmware} differs from the validated versions {:?} — if the module drops out of software mode, the keepalive may have changed on this firmware; please report it",
                        galdeck_hid::ids::VALIDATED_FIRMWARES
                    );
                }
                self.device = Some(DeviceState {
                    deck,
                    firmware,
                    serial,
                });
                self.apply_page();
            }
            Err(galdeck_hid::Error::DeviceNotFound) => {
                log::debug!("device not present, retrying");
            }
            Err(e) => {
                log::warn!("open failed: {e}");
            }
        }
    }

    fn current_page(&self) -> &Page {
        &self.config.pages[self.page_index.min(self.config.pages.len() - 1)]
    }

    fn key_config(&self, key: u8) -> Option<&KeyConfig> {
        self.current_page().keys.iter().find(|k| k.key == key)
    }

    fn encoder_config(&self, encoder: u8) -> Option<&EncoderConfig> {
        self.current_page()
            .encoders
            .iter()
            .find(|e| e.encoder == encoder)
    }

    /// Push the current page's full state to the device.
    fn apply_page(&mut self) {
        let Some(state) = self.device.as_mut() else {
            return;
        };
        let page = &self.config.pages[self.page_index.min(self.config.pages.len() - 1)];
        log::info!("applying page {:?}", page.name);

        let result: std::result::Result<(), galdeck_hid::Error> = (|| {
            state.deck.set_brightness(self.brightness)?;

            for index in Buttons::indices() {
                match page.keys.iter().find(|k| k.key == index) {
                    Some(cfg) => {
                        let background = cfg
                            .color
                            .as_deref()
                            .and_then(Rgb::from_hex)
                            .unwrap_or(DEFAULT_KEY_COLOR);
                        let canvas = render::key(
                            background,
                            cfg.image.as_deref(),
                            cfg.label.as_deref(),
                            self.font.as_ref(),
                        );
                        state.deck.button(index)?.draw(&canvas)?;
                    }
                    None => state.deck.button(index)?.clear()?,
                }
            }

            for index in Encoders::indices() {
                let color = page
                    .encoders
                    .iter()
                    .find(|e| e.encoder == index)
                    .and_then(|e| e.ring.as_deref())
                    .and_then(Rgb::from_hex)
                    .unwrap_or(Rgb::BLACK);
                state.deck.encoder(index)?.ring().set_all(color)?;
            }

            let text = page
                .lcd_text
                .as_deref()
                .or(self.config.lcd_text.as_deref())
                .unwrap_or(&page.name);
            let screen = render::lcd(text, self.font.as_ref());
            state.deck.lcd().draw(&screen)?;
            Ok(())
        })();

        if let Err(e) = result {
            log::warn!("applying page failed, will reconnect: {e}");
            self.device = None;
        }
    }

    fn handle_event(&mut self, event: Event) {
        log::debug!("event: {event:?}");
        match event {
            Event::KeyDown(key) => {
                let (exec, page_target) = match self.key_config(key) {
                    Some(cfg) => (cfg.exec.clone(), cfg.page.clone()),
                    None => (None, None),
                };
                if let Some(cmd) = exec {
                    spawn_action(&cmd);
                }
                if let Some(name) = page_target {
                    self.switch_page(&name);
                }
            }
            Event::EncoderDown(encoder) => {
                if let Some(cmd) = self.encoder_config(encoder).and_then(|e| e.press.clone()) {
                    spawn_action(&cmd);
                }
            }
            Event::EncoderRotate(encoder, delta) => {
                let cfg = self.encoder_config(encoder);
                let cmd = if delta > 0 {
                    cfg.and_then(|e| e.cw.clone())
                } else {
                    cfg.and_then(|e| e.ccw.clone())
                };
                if let Some(cmd) = cmd {
                    // Fast turns coalesce into one report with |delta| > 1;
                    // run the command once per detent (capped) so spins
                    // aren't silently dropped. GALDECK_DELTA carries the
                    // signed total for scripts that prefer one scaled step.
                    let detents = (delta.unsigned_abs() as u32).min(MAX_DETENTS_PER_EVENT);
                    for _ in 0..detents {
                        spawn_action_with_delta(&cmd, delta);
                    }
                }
            }
            _ => {}
        }
    }

    fn switch_page(&mut self, name: &str) -> bool {
        match self.config.pages.iter().position(|p| p.name == name) {
            Some(index) => {
                self.page_index = index;
                self.apply_page();
                true
            }
            None => {
                log::warn!("unknown page {name:?}");
                false
            }
        }
    }

    fn service_control(&mut self) {
        while let Ok(msg) = self.control_rx.try_recv() {
            let response = self.handle_request(msg.request);
            let _ = msg.reply.send(response);
        }
    }

    fn handle_request(&mut self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Ok,
            Request::Status => Response::Status(Status {
                connected: self.device.is_some(),
                firmware: self.device.as_ref().map(|d| d.firmware.clone()),
                serial: self.device.as_ref().map(|d| d.serial.clone()),
                page: self.current_page().name.clone(),
                pages: self.config.pages.iter().map(|p| p.name.clone()).collect(),
                brightness: self.brightness,
            }),
            Request::SetBrightness { percent } => {
                if percent > 100 {
                    return Response::Error {
                        message: "brightness must be 0-100".into(),
                    };
                }
                self.brightness = percent;
                if let Some(state) = self.device.as_mut() {
                    if let Err(e) = state.deck.set_brightness(percent) {
                        self.device = None;
                        return Response::Error {
                            message: e.to_string(),
                        };
                    }
                }
                Response::Ok
            }
            Request::SwitchPage { name } => {
                if self.switch_page(&name) {
                    Response::Ok
                } else {
                    Response::Error {
                        message: format!("unknown page {name:?}"),
                    }
                }
            }
            Request::Reload => match Config::load(&self.config_path) {
                Ok(config) => {
                    self.font = render::load_font(config.font.as_deref());
                    self.brightness = config.brightness;
                    let current = self.current_page().name.clone();
                    self.page_index = config
                        .pages
                        .iter()
                        .position(|p| p.name == current)
                        .unwrap_or(0);
                    self.config = config;
                    self.apply_page();
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: format!("{e:#}"),
                },
            },
        }
    }
}

/// Run a shell command without blocking the engine; a helper thread reaps it.
fn spawn_action(cmd: &str) {
    spawn_action_inner(cmd, None);
}

/// Like [`spawn_action`], exporting the signed rotation delta as
/// `GALDECK_DELTA` for scripts that want one scaled step per event.
fn spawn_action_with_delta(cmd: &str, delta: i8) {
    spawn_action_inner(cmd, Some(delta));
}

fn spawn_action_inner(cmd: &str, delta: Option<i8>) {
    log::info!("exec: {cmd}");
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(cmd);
    if let Some(delta) = delta {
        command.env("GALDECK_DELTA", delta.to_string());
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || match child.wait() {
                Ok(status) if !status.success() => log::warn!("action exited with {status}"),
                Err(e) => log::warn!("waiting on action: {e}"),
                _ => {}
            });
        }
        Err(e) => log::warn!("spawning action failed: {e}"),
    }
}
