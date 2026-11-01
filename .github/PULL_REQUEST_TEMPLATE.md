## What and why

<!-- What does this change, and what problem does it solve? Link the issue if there is one. -->

## Checks

- [ ] `cd rust && cargo test-core-isolated` passes
- [ ] `cd rust && cargo build-app` builds (if the window or QML changed)
- [ ] `scripts/i18n-update.sh --check` passes (if interface strings changed)
- [ ] Tested on: <!-- distribution, desktop, Wayland/X11 -->

<!-- If this touches the privileged helpers (rust/rexon-core/src/privilege/, system_plan.rs, installer.rs), describe what root may now do that it could not before. -->
