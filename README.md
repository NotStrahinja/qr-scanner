# qr-scanner
A simple QR scanner for PC written in Rust

## How it works
It works by capturing two screenshots, a normal one and a dark one.

Then it renders the dark screenshot in borderless fullscreen mode and sets up an event listener.

The event listener listens for a left click. Once it finds it, it will render a rectangle on the dark screenshot that acts like a mask.

After you let go of the left mouse button, the decoded QR contents will be copied to your clipboard.

In case it fails, it won't copy anything.

## Compiling
You can build the app like so:
```console
$ cargo build --release
```
