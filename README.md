# get-mouse-battery-rs

I previously used an OpenRazer-based Python script like the one below in order
to create a KDE Plasmoid widget that shows my mouse's charge level. However,
it's a bit bloated to install the entire OpenRazer suite including its DKMS
driver _and_ an entire userspace Python daemon that's always running, when
literally the only thing I care about is getting the charge level.

The old Python script:

```python
#!/usr/bin/env python3
from openrazer.client import DeviceManager
import argparse

parser = argparse.ArgumentParser(
    usage="%(prog)s [--verbose]",
    description="Gets razer mouse battery charge level."
)
parser.add_argument(
    "-v", "--verbose", action = "store_true"
)

args = parser.parse_args()

device_manager = DeviceManager()
mouse = None

for device in device_manager.devices:
    if args.verbose:
        print(f"Found device: {device.name}")
    if device.name.startswith("Razer Basilisk V3 Pro") and device.battery_level > 0:
        mouse = device

if None == mouse:
    print("N/A")
    exit(0)

charge_status = ""
if mouse.is_charging:
    charge_status = " ⚡"

print("{}%{}".format(mouse.battery_level, charge_status))
```

The Plasmoid would invoke this script every second to give me an always up
to date charge level.

This tiny Rust app uses the `hidapi` crate to directly query the charge level
_directly_ from USB, bypassing all the layers introduced by OpenRazer, thus
allowing me to not need to run OpenRazer at all!

Much of the code to communicate with the device is derived from the OpenRazer
`razermouse` device driver, so as such, this code inherits their GNU GPL v2
license.

