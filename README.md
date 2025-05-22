# `ston`: Serial TTY Over Network

> [!WARNING]
> Early development, breaking changes may occur.

A simple serial tty to network bridge.

Access serial devices via network connections, for example telnet.

## Usage

```text
Usage: ston [OPTIONS] --device <DEVICE>

Options:
  -d, --device <DEVICE>          path to the serial device (eg. /dev/ttyUSB0)
  -b, --baud <BAUD>              baud rate [default: 115200]
  -c, --connection <CONNECTION>  <IP:PORT> listening [default: 0.0.0.0:7070]
  -h, --help                     Print help
  -V, --version                  Print version
```

## Example

```bash
# run with cargo command
cargo r -- -d /dev/tty.usbserial-10

# run with binary build
ston -d /dev/tty.usbserial-10

# custom baud rate and mount point
ston -d /dev/tty.usbserial-10 -b 115200 -c 127.0.0.1:7890
```