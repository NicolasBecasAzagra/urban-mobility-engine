# CI/CD Configuration

**Configuration date:** February 2, 2026  
**Status:** ✅ Active  
**Platform:** GitHub Actions

---

## Overview

The project uses **GitHub Actions** to automate:
- ✅ Unit and integration tests
- ✅ Linting with clippy
- ✅ Format checking (rustfmt)
- ✅ Coverage report generation

---

## CI Workflow

**File:** `.github/workflows/ci.yml`

### Triggers

CI runs automatically on:
- **Push** to branches: `main`, `feat/**`, `fix/**`
- **Pull Requests** targeting `main`

### Jobs

#### 1. **Test Suite** (`test`)
- Runs `cargo test --verbose`
- Runs doc-tests: `cargo test --doc`
- Installs system dependencies (build-essential, clang) for the S2 binding
- Caches Rust dependencies for fast builds

**Failure = blocks merge**

#### 2. **Rustfmt** (`fmt`)
- Checks that the code is formatted: `cargo fmt -- --check`
- Ensures style consistency across the whole project

**Failure = blocks merge**

#### 3. **Clippy** (`clippy`)
- Runs the idiomatic Rust linter
- Command: `cargo clippy --all-targets --all-features -- -D warnings`
- The `-D warnings` flag turns warnings into errors (strict standard)

**Failure = blocks merge**

#### 4. **Code Coverage** (`coverage`)
- Generates a coverage report with `cargo-tarpaulin`
- Uploads results to Codecov (optional, requires a token)
- Output format: XML (cobertura.xml)

**Failure = does NOT block merge** (informational)

---

## Optimizations

### Dependency Caching

The workflow caches:
```yaml
~/.cargo/registry  # Downloaded crates
~/.cargo/git       # Git repositories
target/            # Compiled artifacts
```

**Benefit:**  
- First run: ~5-8 minutes
- Subsequent runs: ~1-2 minutes

### Matrix Strategy (Future)

Currently Ubuntu only. Potential expansion:
```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
```

---

## README Badges

```markdown
[![CI](https://github.com/NicolasBecasAzagra/urban-mobility-engine/actions/workflows/ci.yml/badge.svg)](...)
```

**Possible states:**
- 🟢 **Passing:** All jobs succeeded
- 🔴 **Failing:** At least one job failed
- 🟡 **Running:** CI in progress

---

## Local vs CI Usage

### Local (development)
```bash
cargo test          # Without --verbose
cargo clippy        # Without -D warnings (allows warnings)
cargo fmt           # Formats automatically
```

### CI (strict standard)
```bash
cargo test --verbose           # Full logs
cargo clippy -- -D warnings    # Fails on warnings
cargo fmt -- --check           # Only checks, does not modify
```

**Best practice:** Run the CI commands locally before pushing:
```bash
./scripts/pre-push-check.sh  # TODO: create this script in Phase 2
```

---

## Branch Protection Configuration

To block merges if CI fails, configure in GitHub:

1. **Settings → Branches → Branch protection rules**
2. Add a rule for `main`:
   - ✅ Require status checks to pass before merging
   - ✅ Require branches to be up to date before merging
   - Select checks: `test`, `fmt`, `clippy`

---

## Troubleshooting

### Error: "S2 headers not found"
**Cause:** System dependencies not installed  
**Solution:** Already included in the workflow:
```yaml
- name: Install system dependencies
  run: sudo apt-get install -y build-essential clang
```

### Error: "Cargo.lock conflicts"
**Cause:** Outdated lockfile  
**Solution:** CI always uses the latest compatible version. Locally: `cargo update`

### Coverage job failed
**Cause:** tarpaulin timeout (complex code)  
**Solution:** Increase the timeout in the workflow:
```yaml
run: cargo tarpaulin --timeout 300  # 5 minutes
```

---

## Future Roadmap

### Phase 2-3 (March-April)
- [ ] Add automatic benchmarks (criterion.rs)
- [ ] Integrate codecov for coverage visualization
- [ ] Add a security audit (`cargo-audit`)

### Phase 4 (May)
- [ ] Automatic documentation deployment to GitHub Pages
- [ ] Release automation (tags → GitHub Releases)
- [ ] Automatic changelog with conventional-changelog

---

## References

- **GitHub Actions Docs:** https://docs.github.com/en/actions
- **Rust CI Best Practices:** https://rust-lang.github.io/api-guidelines/
- **cargo-tarpaulin:** https://github.com/xd009642/tarpaulin
- **Codecov:** https://about.codecov.io/

---

**Last updated:** February 2, 2026  
**Maintainer:** Project author
