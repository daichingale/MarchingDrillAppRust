# Audio device endurance evidence

`audio_device_diagnostic` exercises the same CPAL output and lock-free playback clock as DrillForge. The realtime callback only updates atomics: it never allocates, locks, formats, or logs. Sampling, JSON, memory queries, seek/rate/click stress, stall detection, and reopen attempts run outside the callback.

Run a safe five-second hardware check:

```powershell
./scripts/test-audio-endurance.ps1 -Profile short
```

Release candidates require both `-Profile 30m` and `-Profile 2h` on each supported device/sample-rate/driver row. Use `-Audible` only when an audible low-level tone is wanted; tests are muted by default. `-Profile ci` is a deterministic device-free wiring check and is not hardware evidence.

Reports use `drillforge.audio-endurance.v1`, cap observations at 512, and contain callback/underrun/stall/error counts, monotonic-clock violations outside explicit seek epochs, reopen attempts, stress-operation counts, and Windows working-set/private-byte start/peak/end values. Exit 0 means pass, 1 means a test or report failure, 64 means invalid arguments, and 77 means no default device. A device-unplug test is evidenced only when `device_errors` or a stall causes `reopen_attempts > 0`; absence of such an event must not be claimed as recovery proof.
