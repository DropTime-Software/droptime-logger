## Summary

<!-- What does this PR change, and why? One or two sentences is plenty.
     Link the issue it closes: "Closes #123". -->

## Test evidence

<!-- How do you know this works? Paste command output, a screenshot, a screen
     recording, or the steps you ran by hand. For a driver change, that means a
     real or emulated capture — show the roast. -->

## Provenance attestation

<!-- Required for any change to a device driver or a file parser/writer.
     If your PR doesn't touch drivers or formats, check "N/A" and move on. -->

- [ ] **N/A** — this PR does not add or change a device driver or a file-format parser/writer.
- [ ] This driver/format work is **clean-room**. Everything here derives only from sanctioned inputs — vendor protocol docs, the BSD-3-Clause aArtisanQ `commands.txt`, my own hardware captures, our documented key maps and self-generated fixtures, or public behavior documentation. I did **not** port, translate, or paraphrase GPL code from Artisan or any other project, and I have not studied another project's driver source for this machine. A `PROVENANCE.md` is included or updated for the driver.

## Checklist

- [ ] `pnpm test` and `cargo test` pass locally.
- [ ] `cargo fmt` and `cargo clippy` are clean; `tsc` and lint pass.
- [ ] I've read the [Contributing guide](https://github.com/OutsideTheBoxDev/droptime-logger/blob/main/CONTRIBUTING.md).

---

By opening this PR you'll be asked to sign the [CLA](https://github.com/OutsideTheBoxDev/droptime-logger/blob/main/CLA.md) — a one-time comment on your first contribution. It lets Droptime include your work in both the open-source Logger and Droptime's commercial products, while you keep your own copyright. The CLA bot posts the details automatically.
