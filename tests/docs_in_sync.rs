//! Docs ↔ code drift gate.
//!
//! The keybinding table on the docs site lives in `docs/src/data/keymap.ts` and carries a
//! `(vX.Y.Z)` stamp marking the crate version its bindings were last reconciled against. Since
//! every change to this crate bumps `Cargo.toml`, this test fails whenever the code was released
//! without re-stamping the keymap — forcing a deliberate "did the docs need updating?" checkpoint
//! each release. Bumping the stamp is the sign-off that the keybinding docs reflect this version.

use std::fs;

#[test]
fn keymap_doc_version_stamp_matches_crate() {
    let crate_version = env!("CARGO_PKG_VERSION");
    let path = "docs/src/data/keymap.ts";
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("could not read {path}: {err}"));

    // The stamp is the first `(vX.Y.Z)` in the file header comment.
    let stamp = contents
        .split("(v")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .map(str::trim)
        .unwrap_or_else(|| panic!("no `(vX.Y.Z)` version stamp found in {path}"));

    assert_eq!(
        stamp, crate_version,
        "\n\ndocs keymap version stamp is stale: {path} says v{stamp} but Cargo.toml is \
         v{crate_version}.\nReview the keybindings/docs for this release, then update the stamp \
         in {path} (and any changed bindings + README + docs pages) to v{crate_version}.\n"
    );
}
