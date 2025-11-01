# Debug API for External Metric Access

The scheduler can expose a simple HTTP API for accessing current metrics, useful for debugging, monitoring, and integration with tools like Cursor's AI assistant.

## Usage

Start the scheduler with the `--debug-api` flag:

```bash
./scx_gamer --debug-api 8080
```

This starts an HTTP server on `http://127.0.0.1:8080` that exposes scheduler metrics as JSON.

## Endpoints

### `GET /metrics`

Returns the current scheduler metrics as JSON. This includes all thread classification counters, CPU utilization, dispatch statistics, and more.

**Example:**
```bash
curl http://127.0.0.1:8080/metrics
```

**Response:**
```json
{
  "cpu_util": 45,
  "fg_pid": 12345,
  "fg_app": "ArcRaiders.exe",
  "input_handler_threads": 1,
  "gpu_submit_threads": 2,
  "game_audio_threads": 1,
  "system_audio_threads": 0,
  "compositor_threads": 1,
  "network_threads": 0,
  "background_threads": 3,
  ...
}
```

### `GET /health`

Returns health check status and whether metrics are available.

**Example:**
```bash
curl http://127.0.0.1:8080/health
```

**Response:**
```json
{
  "status": "healthy",
  "metrics_available": true
}
```

### `GET /`

Returns API information and available endpoints.

## Cursor AI Integration

The AI assistant in Cursor can query this endpoint directly using `curl` commands. Simply tell it:

> "Query the scheduler metrics from http://127.0.0.1:8080/metrics"

The AI can use the `run_terminal_cmd` tool to execute:
```bash
curl -s http://127.0.0.1:8080/metrics | jq .
```

Or for specific metrics:
```bash
curl -s http://127.0.0.1:8080/metrics | jq '{fg_pid, fg_app, input_handler_threads, gpu_submit_threads, game_audio_threads}'
```

## Metrics Available

The `/metrics` endpoint returns all scheduler metrics including:

- **Thread Classifications:**
  - `input_handler_threads` - Input handler thread count
  - `gpu_submit_threads` - GPU submit thread count
  - `game_audio_threads` - Game audio thread count
  - `system_audio_threads` - System audio thread count
  - `compositor_threads` - Compositor thread count
  - `network_threads` - Network thread count
  - `background_threads` - Background thread count

- **Game Detection:**
  - `fg_pid` - Foreground game process ID
  - `fg_app` - Foreground game application name
  - `fg_fullscreen` - Fullscreen flag

- **Performance Metrics:**
  - `cpu_util` - CPU utilization percentage
  - `cpu_util_avg` - Average CPU utilization (EMA)
  - `frame_hz_est` - Estimated frame rate (Hz)
  - `direct` - Direct dispatches
  - `shared` - Shared dispatches
  - `migrations` - CPU migrations
  - `mig_blocked` - Blocked migrations

- **Input Statistics:**
  - `input_trig` - Input trigger count
  - `input_trigger_rate` - Input trigger rate
  - `continuous_input_mode` - Continuous input mode flag
  - `continuous_input_lane_keyboard` - Keyboard lane active
  - `continuous_input_lane_mouse` - Mouse lane active

- **Ring Buffer Stats:**
  - `ringbuf_overflow_events` - Ring buffer overflow count
  - `rb_queue_dropped_total` - Userspace queue drops
  - `rb_queue_high_watermark` - Queue depth

- **And many more...**

## Notes

- The API only binds to `127.0.0.1` (localhost) for security
- Metrics update every 1 second when debug API is enabled
- The API server runs in a separate thread and doesn't impact scheduler performance
- Use `Ctrl+C` to stop the scheduler, which will also stop the API server
- The endpoint returns pretty-printed JSON by default for readability
