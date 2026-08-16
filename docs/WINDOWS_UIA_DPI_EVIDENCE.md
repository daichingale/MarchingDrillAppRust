# Windows UIA and DPI evidence

`scripts/collect-windows-uia-dpi-evidence.ps1` separates deterministic CI smoke from the real-device gate.

```powershell
# CI-safe: AccessKit tree at logical 100/150/200% plus keyboard focus contract
./scripts/collect-windows-uia-dpi-evidence.ps1 -Mode Headless

# Interactive Windows release-lab gate: bounded UIA tree, native window DPI,
# monitor bounds, focus metadata, and PNGs
./scripts/collect-windows-uia-dpi-evidence.ps1 -Mode Manual
```

Outputs are bounded to 400 UIA nodes by default and written under `artifacts/windows-uia-dpi/`. Exit code `0` means pass, `1` means fail, and `2` is an explicit skip when the manual gate has no interactive Windows desktop.

The requested 100/150/200% values are DrillForge logical zoom factors. `window_dpi` is separately measured with `GetDpiForWindow`; the report never presents simulated scaling as physical-monitor evidence. For mixed-DPI certification, move the window between monitors with different Windows scaling, rerun the manual gate, and inspect the recorded monitor/window bounds and PNGs. The harness reads UIA state but does not inject Tab or activate controls; keyboard behavior is covered by the deterministic headless contract and remains a human assistive-technology gate on the real device.
