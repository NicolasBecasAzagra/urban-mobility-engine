# Development Environment Setup Guide

**Project:** Urban Mobility Analysis Engine in Rust  
**Last updated:** February 2, 2026

---

## 📋 System Requirements

### 1. Rust Toolchain
```bash
# Install Rust (if you don't have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Check the version (minimum: 1.70+)
rustc --version
cargo --version
```

### 2. System Dependencies for S2

The `s2` crate (a binding to Google's C++ library) requires:

#### macOS
```bash
# C++ compiler (Clang via Xcode Command Line Tools)
xcode-select --install

# Verify the installation
clang++ --version
```

#### Linux (Ubuntu/Debian)
```bash
sudo apt update
sudo apt install -y build-essential cmake clang
```

#### Linux (Fedora/RHEL)
```bash
sudo dnf install -y gcc-c++ cmake clang
```

**Why these dependencies?**
- The `s2` crate uses **FFI (Foreign Function Interface)** to call C++ code
- It needs a **C++ compiler** (clang++ or g++) to build the binding
- Some advanced geometric functions live in Google's native library

---

## 🔧 Environment Verification

### Step 1: Clone the Repository
```bash
git clone [YOUR_REPO_URL]
cd TFG
```

### Step 2: Create the Local Configuration
```bash
# Copy the configuration template
cp config.example.toml config.toml

# (Optional) Adjust values in config.toml for your environment
```

### Step 3: Build the Project
```bash
# First build (downloads dependencies and compiles S2)
cargo build

# If you see this message, everything is correct:
# "Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs"
```

**Troubleshooting:**
- **Error: `fatal error: 's2/s2latlng.h' file not found`**  
  → The C++ compiler is missing. Install `clang++` for your system (see above).

- **Error: `linker 'cc' not found`**  
  → The system linker is missing. On Ubuntu: `sudo apt install build-essential`

### Step 4: Run the Tests
```bash
# Run all unit tests
cargo test

# Expected output:
# test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Step 5: Run the CLI
```bash
# Run the main binary
cargo run

# Expected output:
# INFO tfg: Urban Mobility Analysis Engine - Phase 0
# INFO tfg: Configuration loaded:
# INFO tfg:   - Base level S2: 15
# ...
```

---

## 🛠️ Recommended Development Tools

### 1. IDE: Visual Studio Code + rust-analyzer

**Installation:**
```bash
# Install VS Code (if you don't have it)
# macOS: brew install --cask visual-studio-code
# Linux: snap install code --classic

# Install the rust-analyzer extension
code --install-extension rust-lang.rust-analyzer
```

**Recommended configuration (`.vscode/settings.json`):**
```json
{
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.cargo.features": "all",
  "editor.formatOnSave": true,
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

### 2. Linter: Clippy

Clippy detects common errors and suggests idiomatic improvements.

```bash
# Install clippy (included in rustup)
rustup component add clippy

# Run the linter
cargo clippy

# For CI/CD (fail on warnings):
cargo clippy -- -D warnings
```

### 3. Formatter: rustfmt

```bash
# Install rustfmt (included in rustup)
rustup component add rustfmt

# Format all the code
cargo fmt

# Check without modifying (useful for CI)
cargo fmt -- --check
```

### 4. Documentation

```bash
# Generate HTML documentation
cargo doc --open

# This will open the documentation in the browser
# Includes docs for your modules + all dependencies
```

---

## 📦 Project Structure

```
TFG/
├── Cargo.toml              # Project manifest and dependencies
├── config.example.toml     # Configuration template (versioned)
├── config.toml             # Local configuration (NOT versioned)
├── docs/                   # Technical documentation
│   ├── ci-cd-setup.md
│   ├── data-sources.md
│   ├── development-setup.md
│   └── gps-data-schema.md
├── src/
│   ├── main.rs             # CLI entry point
│   ├── lib.rs              # Main library
│   ├── config.rs           # Configuration system
│   ├── ingestion/          # GPS data ingestion module
│   ├── geospatial/         # S2 gridding module
│   ├── metrics/            # delay_factor computation
│   └── utils/              # Utilities (logging, errors)
└── data/                   # GPS data (not versioned)
    ├── gps_raw/            # Input CSV files
    └── processed/          # Engine output
```

---

## 🧪 Development Workflow

### Typical workflow:
1. **Create a feature branch:**
   ```bash
   git checkout -b feat/feature-name
   ```

2. **Develop with TDD:**
   ```bash
   # Write the tests first
   # Implement the code
   cargo test --lib  # Library tests only
   cargo test        # All tests
   ```

3. **Verify quality:**
   ```bash
   cargo fmt         # Format code
   cargo clippy      # Linting
   cargo test        # Tests
   cargo build --release  # Optimized build (optional)
   ```

4. **Commit with Conventional Commits:**
   ```bash
   git add .
   git commit -m "feat(geospatial): implement S2 cell indexing"
   ```

5. **Merge into main:**
   ```bash
   git checkout main
   git merge feat/feature-name
   ```

---

## 🔍 Debugging

### View detailed logs:
```bash
# Temporarily change the logging level
RUST_LOG=tfg=debug cargo run

# Or permanently in config.toml:
# [logging]
# level = "debug"
```

### Interactive debugger (LLDB on macOS/Linux):
```bash
# Install CodeLLDB in VS Code
code --install-extension vadimcn.vscode-lldb

# Configure launch.json for Rust
# (VS Code offers an automatic template)
```

---

## 📚 References

- **The Rust Book:** https://doc.rust-lang.org/book/
- **Rust by Example:** https://doc.rust-lang.org/rust-by-example/
- **S2 Geometry (Google):** http://s2geometry.io/
- **`s2` crate (docs):** https://docs.rs/s2/latest/s2/
- **Serde (serialization):** https://serde.rs/
- **Anyhow (error handling):** https://docs.rs/anyhow/

---

## 🚨 Common Troubleshooting

### Problem: "Cargo.lock conflicts on merge"
**Solution:**
```bash
git checkout --theirs Cargo.lock  # Use the remote branch's version
cargo build                       # Regenerate the lockfile
git add Cargo.lock
git commit
```

### Problem: "compilation takes forever"
**Cause:** The first S2 compilation is slow (C++ binding).  
**Solution:** Normal. It compiles once, then it is incremental (fast).

### Problem: "cannot find -lstdc++"
**Cause:** The C++ standard library is missing.  
**Solution (Ubuntu):**
```bash
sudo apt install libstdc++-12-dev
```

---

## ✅ Complete Setup Checklist

- [ ] Rust installed (1.70+)
- [ ] C++ compiler available (`clang++` or `g++`)
- [ ] `cargo build` runs without errors
- [ ] `cargo test` passes all tests (3/3)
- [ ] `cargo run` shows the configuration correctly
- [ ] `cargo clippy` reports no warnings
- [ ] `cargo fmt` formats without changes (code already formatted)
- [ ] VS Code + rust-analyzer configured
- [ ] Documentation generated (`cargo doc --open`)

---

**Last verification:** February 2, 2026  
**Test system:** macOS 15.2 with Rust 1.87.0
