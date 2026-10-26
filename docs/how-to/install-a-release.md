# Install a release

This guide installs a ready-made build of open-powerz: the desktop app or
the `powerz` command line tool. Releases live on GitHub:
<https://github.com/nesquikm/open-powerz/releases/latest>.

The builds are not signed yet. macOS and Windows warn about them the first
time you open them. The steps below show how to get past each warning.

## What is in a release

Download only the files you need from the release's **Assets** list.
`<version>` is the release number, for example `0.6.0`.

| file | what |
|---|---|
| `open-powerz_<version>_universal.dmg` | desktop app for macOS (Apple silicon and Intel) |
| `open-powerz_<version>_x64_en-US.msi` | desktop app for Windows, Windows Installer package |
| `open-powerz_<version>_x64-setup.exe` | desktop app for Windows, setup program |
| `open-powerz_<version>_amd64.AppImage` | desktop app for Linux x86_64, runs without installing |
| `open-powerz_<version>_amd64.deb` | desktop app for Debian and Ubuntu x86_64 |
| `powerz-<version>-macos-universal.tar.gz` | `powerz` CLI for macOS, with LICENSE and README |
| `powerz-<version>-linux-x86_64.tar.gz` | `powerz` CLI for Linux x86_64, with LICENSE and README |
| `powerz-<version>-windows-x86_64.zip` | `powerz` CLI for Windows x86_64, with LICENSE and README |
| `SHA256SUMS.txt` | SHA-256 checksums of every file above |

On Windows, pick either the `.msi` or the setup `.exe`. Both install the
same app. A pre-release (such as `v0.6.0-rc.1`) has only the setup `.exe`,
because a Windows Installer package needs a plain numeric version.

## Check the download

Download `SHA256SUMS.txt` into the same folder as your files, then check
them.

On macOS:

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

On Linux:

```bash
sha256sum -c --ignore-missing SHA256SUMS.txt
```

Each file you downloaded should print `OK`. Files you did not download are
skipped.

On Windows, in PowerShell:

```powershell
Get-FileHash open-powerz_<version>_x64_en-US.msi -Algorithm SHA256
```

Compare the printed hash with the line for that file in `SHA256SUMS.txt`
(case does not matter). If a check fails, download the file again.

## macOS

The app is not signed or notarized, so Gatekeeper blocks it the first time.

1. Open the `.dmg` and drag **open-powerz** to **Applications**.
2. Open the app from **Applications**. macOS says it cannot check the app.
   Click **Done** (or **OK**).
3. Open **System Settings → Privacy & Security**. Scroll down to the message
   about open-powerz and click **Open Anyway**. Confirm with your password.

The app now opens normally. Instead of steps 2 and 3 you can clear the
quarantine flag in Terminal:

```bash
xattr -dr com.apple.quarantine /Applications/open-powerz.app
```

For the CLI, unpack the archive and run the tool:

```bash
tar -xzf powerz-<version>-macos-universal.tar.gz
./powerz --help
```

If macOS blocks `powerz`, clear its quarantine flag and try again:

```bash
xattr -d com.apple.quarantine powerz
```

Move `powerz` to a folder on your `PATH` (for example `/usr/local/bin`) to
run it from anywhere.

## Windows

The installers are not signed, so SmartScreen shows **Windows protected your
PC** the first time.

1. Run the `.msi` or the setup `.exe`.
2. In the SmartScreen window, click **More info**, then **Run anyway**.
3. Follow the installer. open-powerz then appears in the Start menu.

For the CLI, unpack `powerz-<version>-windows-x86_64.zip` (right-click →
**Extract All**) and run `powerz.exe --help` from a terminal in that folder.
If SmartScreen stops it, use **More info → Run anyway** again.

## Linux

Use the AppImage on any x86_64 distribution:

```bash
chmod +x open-powerz_<version>_amd64.AppImage
./open-powerz_<version>_amd64.AppImage
```

Or install the `.deb` on Debian or Ubuntu:

```bash
sudo apt install ./open-powerz_<version>_amd64.deb
```

For the CLI:

```bash
tar -xzf powerz-<version>-linux-x86_64.tar.gz
./powerz --help
```

Then install the udev rule as described in
[Linux setup](../linux.md#1-install-the-udev-rule), so the meter can be opened
without root. Without the rule, the app and the CLI cannot open the meter.
The same page explains the kernel's `powerz` hwmon driver, which can hold the
meter's bulk interface.

## About signing

Signed and notarized builds come later. Until then the warnings above are
expected. If you prefer, build from source as described in the
[README](../../README.md#building).
