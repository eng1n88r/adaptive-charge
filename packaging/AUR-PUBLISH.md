# Publishing to the AUR

Blocked until AUR account registration reopens (paused 2026-09 for an
automated-signup wave; watch aur-general / Arch news). Once you have an
account with an SSH key added:

```sh
# one repo per package on the AUR
git clone ssh://aur@aur.archlinux.org/adaptive-charge-bin.git /tmp/aur-bin
cp packaging/bin/PKGBUILD packaging/bin/.SRCINFO packaging/adaptive-charge.install /tmp/aur-bin/
# fix the install= path: on the AUR the file sits next to the PKGBUILD
sed -i 's|install=../adaptive-charge.install|install=adaptive-charge.install|' /tmp/aur-bin/PKGBUILD
cd /tmp/aur-bin && makepkg --printsrcinfo > .SRCINFO
git add -A && git commit -m "adaptive-charge-bin 1.0.0-1" && git push
```

Same procedure with `packaging/source/` for the `adaptive-charge` source
package (optional; the -bin package is what most users want).

## New releases

1. Bump version in Cargo.toml, commit, `git tag vX.Y.Z`, push tag
2. Rebuild: `make build`, re-create the tarball (binary + /usr-pathed unit +
   sudoers + LICENSE + README), `gh release create vX.Y.Z <tarball>`
3. Update pkgver + sha256sums in both PKGBUILDs, regenerate .SRCINFO, push to AUR
