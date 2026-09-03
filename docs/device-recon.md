# Device recon runbook (do this first, with the keyboard plugged in)

Goal: capture your unit's ground truth before anything writes to it, and
establish which firmware you're on. Everything public is validated only on
firmware **3.06.005** — a newer firmware may use a different keepalive, so
**do not update firmware until support is confirmed working on your unit**
(there is no known downgrade path).

## 1. Enumerate

```sh
lsusb | grep -i 1b1c
# expect three ids: 2b19 (hub), 2b0c (keyboard), 2b18 (stream deck module)

lsusb -v -d 1b1c:2b18 > captures/lsusb-2b18.txt 2>&1
lsusb -v -d 1b1c:2b0c > captures/lsusb-2b0c.txt 2>&1
lsusb -v -d 1b1c:2b19 > captures/lsusb-2b19.txt 2>&1
```

## 2. HID report descriptors

`hid-decode` ships in the `hid-tools` package (Debian/Ubuntu/Arch/Fedora).

```sh
for h in /sys/kernel/debug/hid/*1B1C:2B18*/rdesc /sys/kernel/debug/hid/*1b1c*2b18*/rdesc; do
    sudo cat "$h" 2>/dev/null
done > captures/rdesc-2b18.txt

# or, per hidraw node:
ls /dev/hidraw*
sudo hid-decode /dev/hidrawX   # repeat for each node of the 2b18 device
```

## 3. udev rule, then firmware version

```sh
sudo cp udev/70-galdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
# replug the keyboard, then:
cargo run -p galdeck-cli -- detect
```

`detect` prints the module's firmware version and serial without changing
any state. **Record the firmware version in captures/ and in any issue you
open.**

## 4. Full protocol checkout

```sh
cargo run -p galdeck-hid --example verify           # ~1 minute, interactive
cargo run -p galdeck-hid --example verify -- --soak # + 6-minute keepalive soak
```

If `verify` passes on your firmware, please report it (firmware version +
distro + kernel) — every confirmation extends the validation matrix in
[protocol.md](protocol.md). If it fails at the input/event stage on
firmware ≥ 3.06.006, you are probably seeing the changed-keepalive issue;
capture evidence (step 5) and open an issue.

## 5. If something disagrees with the docs: capture traffic

On the Linux host (captures both directions of everything):

```sh
sudo modprobe usbmon
# find the bus/device of 1b1c:2b18 in lsusb, then e.g. bus 3:
sudo wireshark -i usbmon3
```

For comparing against the official software: pass through **only the 2b18
device** to a Windows VM (qemu/VirtualBox USB passthrough), run the Elgato
Stream Deck app there, and capture with usbmon on the Linux host. Replug
the device inside the VM to capture the full init sequence. Change one
thing at a time per capture; save as `.pcapng` into `captures/` with a note
of what you did.
