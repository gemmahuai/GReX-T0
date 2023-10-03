# GReX-T0

First stage processing for the GReX Telescope.

## Structure

This program does quite a bit, depicted by the following chart. Each task has its own thread and is pinned to CPU cores, although the average load per core should be less than 80%.
Tokio handles the less critical async tasks, such as waiting for the dump signal and hosting the metrics webserver.
More implementation details to come.

```mermaid
graph TD
    A[UDP Capture]
    A -->|8Gpbs| D[Downsample]
    D -->|2Gbps| F[Exfil]
    D -->|8Gbps| E[Voltage ringbuffer]
    G[UDP Voltage dump triggering] --> E

    A -->|Capture Statistics| K[Monitoring Webserver]
    L[FPGA Metrics] -->|Spectrum Integrations| K

    H[Program Entry Point] --> I[FPGA Control and timing]
    I --> J[Task spawning]
```

## Usage

For GReX - the default command line args should be sufficient, but use the `--help` argument to list them all.
