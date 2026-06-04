# fancurve — MS-01 fan controller

Background service + web UI for controlling the two chassis fans of a
MinisForum MS-01 running Linux. A Rust (axum) server reads temperatures from
hwmon sysfs, drives the fans via the nct6798 Super I/O chip, and serves a
Svelte frontend with an interactive, draggable fan-curve editor.

## How it works

- **Hardware discovery by chip name**, not hwmon index (`hwmon7` can change
  between boots): the fan controller is found as `nct67*`, the CPU sensors as
  `coretemp` (package + all cores) and the NVMe drives as `nvme` (Composite).
  Temperatures are read directly from sysfs — no `sensors` subprocess. NVMe
  model names are looked up once at startup via `smartctl -i` (sysfs
  `device/model` as fallback).
- **Control temperature** = CPU package temperature only. Core and NVMe
  temperatures are displayed but do not influence the fans — NVMe composite
  readings have 1 °C resolution (integer Kelvin per the NVMe spec) and react
  too slowly to be useful control inputs.
- **Per-fan modes**:
  - `auto` — hardware control. The `pwmN_enable` value observed at service
    startup (Smart Fan IV = `5` on this board) is restored. Writing `0`
    would not work here: on the nct6775 driver `0` means "full speed, no
    control" — restoring the original value is what actually returns the fan
    to BIOS automatic control.
  - `curve` — `pwmN_enable=1` (manual) and the duty follows the configured
    curve with linear interpolation between points, clamped at the ends.
  - `manual` — fixed duty cycle.
- **Failsafes**: if no temperature can be read in curve mode the fans go to
  100%; on shutdown (SIGTERM/Ctrl-C) all fans are returned to hardware auto.
- **Config** is persisted as JSON at `/etc/fancurve/config.json` (override
  with `FANCURVE_CONFIG`), written atomically, validated on load and on every
  API update.

## Web UI

`http://<host>:8090` (override with `FANCURVE_LISTEN`, e.g. `127.0.0.1:9000`).

- Live temperatures polled every 2 s: CPU package (the control input) with
  all per-core temps, and a separate display-only NVMe section showing each
  drive's model name. Fan RPM / duty per fan.
- Per-fan mode switch (Auto / Curve / Manual) and manual speed slider.
- Interactive curve editor: drag points, double-click empty space to add a
  point, double-click a point to remove it. The orange marker shows the
  current operating point; the dashed line marks the ≈16 % duty below which
  the fans don't spin (raw PWM ≈ 40).
- Changes apply only on **Save & Apply** and are then persisted.

## API

| Method | Path          | Description                              |
|--------|---------------|------------------------------------------|
| GET    | `/api/status` | Temperatures, RPM, duty, mode per fan    |
| GET    | `/api/config` | Current configuration                    |
| PUT    | `/api/config` | Validate, persist and apply a new config |

## Building

Toolchains (Rust + Node) are self-contained in `.toolchain/` — nothing is
installed system-wide (see `UNINSTALL.md`).

```bash
./build.sh     # builds frontend, embeds it into the release binary
```

The release binary is fully self-contained (static assets embedded via
rust-embed): `server/target/release/fancurve`.

## Packaging & installing

```bash
./package.sh   # builds frontend + server and produces a .deb
./install.sh   # package.sh + apt install
```

The package (`server/target/debian/fancurve_*.deb`, built with cargo-deb)
contains the self-sufficient binary, the systemd unit and this README;
`postinst` enables and starts the service, `apt purge` also removes
`/etc/fancurve`. `smartmontools` is a Recommends (used for NVMe model names;
there is a sysfs fallback).

The service starts with whatever modes are saved in the config — fresh
installs default to `auto` for both fans, so nothing changes until you pick
`curve`/`manual` in the UI.

## Development

```bash
# terminal 1: server (also serves the API)
cd server && cargo run
# terminal 2: frontend with hot reload, proxies /api to :8090
cd frontend && npm run dev
```

Run server tests with `cd server && cargo test`.

## AI usage disclaimer

This project was entirely vibecoded using Claude.

## License

Released into the public domain under [The Unlicense](LICENSE).
