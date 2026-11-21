# Contributing to AfterTone

Thank you for contributing to AfterTone! Follow these guidelines to ensure an efficient development and review process.

## Proposing Contributions
- Substantial features, architectural changes, or UI redesigns should normally be discussed before implementation. Open an issue on GitHub to outline your proposal.
- Straightforward bug fixes, documentation improvements, or minor corrections can be submitted directly as a Pull Request.

## Branch and Pull Request Workflow
- Fork the repository and create a descriptive branch (e.g., `fix/streaming-buffer`, `feature/lyrics-glow`).
- Keep pull requests focused on a single change or bug fix.
- Provide a clear PR description explaining what was changed and why.
- Include testing and validation details with your PR.
- Ensure all tests pass (`cargo test`).

## Development Setup
- **Rust**: Rust 1.78+ (stable toolchain)
- **JDK**: Java Development Kit 21
- **Windows Build Tools** (for Windows desktop deployment)

## Build and Test Commands
Run Cargo commands from the repository root:

```bash
# Run unit and integration tests
cargo test

# Build release binary
cargo build --release

# Build Windows Installer (Inno Setup)
powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
```

## Licensing
AfterTone is licensed under the **GNU General Public License v3.0 (GPLv3)**. All contributed code must be strictly compatible with GPLv3.
