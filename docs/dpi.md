# Application classification (nDPI)

*Optional, off by default (feature 003). It builds on [packet capture](routeros.md#packet-capture-connection-naming).*

Packet capture names connections that carry a server name: TLS, QUIC, HTTP and DNS. Much traffic has no name, though: BitTorrent, WireGuard and OpenVPN, games such as Steam, voice and video calls (STUN/RTP), remote desktop, and services on unusual ports. Without classification, tmon can only guess these from the port.

With classification on, tmon passes the captured connection-setup packets to **`tmon-dpi`**, a small sidecar process built on [nDPI](https://github.com/ntop/nDPI). `tmon-dpi` names the application (for example `BitTorrent`, `WireGuard`, `Steam` or `Zoom`) and says how sure it is. The application then shows up in:

- **Flows**: an app column and an app filter. Guesses have a dashed border.
- the host drill-down: *Top applications*;
- the dashboard: *Applications → By app*;
- the CSV export: `app` and `app_confident` columns;
- `/metrics`, as `tmon_dpi_*` series.

Rules can match on `app` and `app_category`. Built-in rules map applications to tmon categories. A new **File Sharing** category covers peer-to-peer traffic. A confident detection gives *detected* confidence; a guess gives *inferred*.

**Why a separate process, and why optional.** nDPI is a large C library that parses untrusted packets. In its own process, a crash, hang or memory bug in nDPI can never stop tmon or lose flow records: tmon restarts the sidecar and keeps running. The main `tmon` binary stays pure Rust and never links nDPI. Most users need neither the extra binary nor the LGPL obligations, so the feature is off unless you enable it.

## Enabling it

Classification needs packet capture (`[capture] enabled = true`). It sees exactly the packets that capture receives.

### Docker

Use the `-dpi` image variant. It is the base image plus `/usr/local/bin/tmon-dpi` and `libndpi`:

```bash
docker build -f deploy/Dockerfile --target tmon-dpi -t tmon:dpi .
```

### Native

```bash
just dpi-build      # builds nDPI 6.0 into backend/dpi/target/ndpi, then tmon-dpi
# → backend/dpi/target/release/tmon-dpi (rpath points at the bundled libndpi)
```

Packages: install `tmon-dpi` next to `tmon`. `binary` may be a name looked up in `PATH`.

### Configuration

```toml
[capture]
enabled = true
# ...

[dpi]
enabled = true
binary = "tmon-dpi"              # or an absolute path
max_flows = 50000                # connections tracked by the classifier
max_packets_per_conn = 32        # packets forwarded per connection, then give up
idle_timeout_secs = 900
memory_limit_mb = 192            # classifier memory budget (RLIMIT_AS = this + 64 MB)
queue_capacity = 16384           # packets queued to the classifier
queue_max_bytes = 16777216
```

The full reference, ranges and validation errors are in [contracts/config.md](../specs/003-ndpi-app-classification/contracts/config.md). If the binary does not exist, tmon still starts. It runs without classification and shows the `dpi_not_found` warning.

### Router: the reply direction (recommended)

Many applications are recognized only from the server's answer. Examples are the WireGuard handshake response, the RDP connection confirm, and the second half of the BitTorrent handshake. Add the reply-direction capture rule from [routeros.md → Reply direction](routeros.md#reply-direction-for-application-classification). It roughly doubles the capture bandwidth, to about 2–6 Mbit/s on a busy home network.

### Verify

1. **Status → Application classification** shows `running`, the nDPI version, and increasing *packets* and *classified* counters.
2. Start a BitTorrent download or a WireGuard tunnel from a LAN device. Within one flow export interval, its flows in **Flows** show the app and the *File Sharing* or *VPN/Tunnels* category.

## How results are used

- **Naming wins**: a host name from capture (TLS/QUIC/HTTP/DNS) is never replaced. The classifier's host name, for example from an RDP cookie, is used only when nothing else named the connection. It shows source `dpi`.
- **Confidence**: nDPI's `DPI`, `DPI_CACHE` and `CUSTOM_RULE` results are *confident*. Port, IP and partial matches are *guesses*. Generic carriers (TLS, QUIC, HTTP, DNS) are never reported as the application.
- **Late answers**: when a classification arrives after its flow was stored, tmon fills it in within about 10 s. This applies to flows from the last 15 minutes.
- **Rule priority**: user rules (≥ 1,000,000) beat everything, and hostname rules (900k) beat application rules. Named apps sit at 860k, nDPI category fallbacks at 850k. Application rules beat ASN, CIDR and port rules (≤ 800k).
- The application breakdown and the drill-down's top applications come from raw flows. They cover the raw retention window (7 days by default).

## Sandbox

`tmon-dpi` starts with fd 3 (its protocol socket) and a clean environment. After nDPI has initialized, it restricts itself in this order. Each step is logged as `tmon-dpi: sandbox <step>: applied|unavailable`, and tmon forwards these lines to its log.

The sandbox **fails closed**. If any step except Landlock fails, the sidecar logs `refusing to classify without the sandbox` and exits before reading a packet, and tmon reports it as a crash. Landlock is optional because it depends on the kernel version, and seccomp already forbids opening files.

| Step | Effect |
|---|---|
| `PR_SET_PDEATHSIG` | dies with tmon |
| `no_new_privs` | no privilege gain via exec |
| `drop_root` | if started as root: `setgroups([])`, then uid and gid 65534 (`nobody`) |
| `landlock` | denies all filesystem access (best effort; needs Linux ≥ 5.13) |
| `rlimit_as` | address space capped at what is mapped after nDPI starts (libraries and nDPI's tables) + `memory_limit_mb` + 128 MB |
| `seccomp` | allow-list of the syscalls the I/O loop needs; anything else **kills the process**. That includes `socket`, `connect`, `open*`, `execve`, `kill`/`tgkill` (when tmon is not root, both run as the same user) and `sendmsg` (fd passing). `mmap`/`mprotect` are allowed only without `PROT_EXEC`, and `fcntl` only as `F_GETFD` (debug builds check a descriptor before closing it). File opens (`open`/`openat`) fail with `EACCES` instead of killing the process: the C library may try to open files such as the timezone database while nDPI runs, and it handles the error |

To check a running sidecar:

```bash
pid=$(pgrep -f tmon-dpi)
grep -E 'NoNewPrivs|Seccomp' /proc/$pid/status   # NoNewPrivs: 1, Seccomp: 2
```

The test suite (`backend/dpi/tmon-dpi/tests/sandbox.rs`) checks that network access kills the process with SIGSYS, and that file opens are denied without killing it.

When running tmon in Docker, add `--cap-drop=ALL --security-opt=no-new-privileges` and keep the default seccomp profile, as defense in depth. With classification on, the sidecar is the only part of tmon that runs C code on network data. If it is compromised, the attacker can at most invent or change application labels (names are limited to printable ASCII without markup characters) or flood tmon. Floods are rate-limited, and a flooding sidecar is restarted.

## Memory sizing

| Part | Bound |
|---|---|
| Classifier flows | `max_flows` (LRU). Each nDPI flow is about 1–1.5 KB plus buffers |
| Classifier heap | every nDPI allocation is counted. Above `memory_limit_mb`, the least recently used flows are freed until usage is under 90 % (`evicted{memory}`) |
| Hard backstop | `RLIMIT_AS = memory_limit_mb + 64 MB`. Hitting it crashes the sidecar, which tmon restarts |
| tmon → classifier queue | `queue_capacity` packets and `queue_max_bytes`. When full, packets are skipped (`dropped_queue`), never flows |
| tmon application table | `2 × max_flows` entries of about 120 B, with the idle timeout as TTL |

The defaults (50k flows, 192 MB) fit a busy home or small-office network. Halve both on small hosts (e.g. a Raspberry Pi with 1 GB).

## Troubleshooting

<a id="troubleshooting"></a>

| Warning | Meaning | Fix |
|---|---|---|
| `dpi_not_found` | `dpi.binary` does not exist or is not executable | Install `tmon-dpi`, use the `-dpi` image, or fix `binary`. tmon runs without classification meanwhile |
| `dpi_incompatible` | The sidecar speaks another protocol version | Install `tmon` and `tmon-dpi` from the same release |
| `dpi_failed` | 5 crashes or hangs within 10 min; tmon gave up | See the `tmon-dpi:` lines in the tmon log. Restart tmon to retry. Lower `memory_limit_mb` pressure (fewer `max_flows`) if the cause was memory |
| `dpi_restarting` | ≥ 2 restarts in 10 min | Usually transient. If it persists, see `dpi_failed` |
| `dpi_dropping` | > 5 % of packets were skipped over 15 min because the classifier could not keep up | Raise `dpi.queue_capacity`, lower `max_packets_per_conn`, or narrow the capture rules |

Other symptoms:

- **Everything is still "guessed"**: check that the capture card shows packets, and add the reply-direction rule.
- **Status shows `starting` forever**: the sidecar did not send its hello within 5 s. Run `tmon-dpi --fd 3` by hand: it exits immediately when fd 3 is not a socket and prints why.

## Licensing

- **tmon** (including `tmon-dpi`'s own code) is MIT-licensed.
- **nDPI** is **LGPL-3.0-or-later**. `tmon-dpi` links it **dynamically** (`libndpi.so`). The base `tmon` binary and image do not contain nDPI at all.
- If you **redistribute** `tmon-dpi` or the `-dpi` image, you must:
  1. ship the nDPI license text (the image has it in `/usr/share/doc/tmon-dpi/nDPI-COPYING`);
  2. provide the corresponding nDPI source, or a written offer for it. The image's `NOTICE-nDPI` names the exact tag, commit and repository;
  3. allow users to replace `libndpi.so` with a modified, compatible build. Dynamic linking satisfies this; do not hinder it.
  4. **not** link nDPI statically into `tmon-dpi` unless you also provide the object files needed to relink it.
- Using tmon privately, without redistribution, carries no extra obligations.

This is a summary, not legal advice. The license texts are authoritative.

## Writing another classifier

The sidecar protocol is small, versioned and documented: [contracts/dpi-ipc.md](../specs/003-ndpi-app-classification/contracts/dpi-ipc.md). It uses a SOCK_SEQPACKET socket on fd 3, one message per packet, `Hello`, `Classification` and `Stats`. Any program that speaks it can be set as `dpi.binary`. One example is the pure-Rust test classifier `backend/crates/tmon-dpi-client/examples/fake_dpi.rs`.
