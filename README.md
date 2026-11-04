# Billy Jean

Billy Jean is a small desktop app for macOS that writes invoices as PDF files.

The window has two parts. On the left is a form with the panels Sender, Bill to, Invoice details, Line items, Totals, Notes and Bank details. On the right is a live preview of the PDF, which updates as you type. When you are done, click **Export PDF** and choose where to save the file. Before it exports, Billy Jean checks that the required fields are filled in, there is at least one line item, and the IBAN is valid.

- Line totals and invoice totals (net, VAT, gross) are calculated for you.
- The VAT rate starts at 0%. At 0% the invoice carries the German §19 UStG (small business) note. Any other rate recalculates VAT and gross and replaces that note.
- The PDF is generated inside the app. You do not need any other software.
- Your sender and bank details are remembered, so they are filled in next time.

Billy Jean does not save invoices, keep an invoice history or client address book, number invoices automatically, or support currencies other than EUR.

## Install

You need macOS 11 (Big Sur) or later, on Apple silicon or Intel.

### With Homebrew

```bash
brew tap pravinbashyal/billy-jean
brew install --cask billy-jean
```

This puts **Billy Jean** in your Applications folder. To update it later:

```bash
brew update
brew upgrade --cask billy-jean
```

### Without Homebrew

1. Open the [latest release](https://github.com/pravinbashyal/billy-jean/releases/latest).
2. Download `Billy-Jean-<version>-arm64.dmg` if you have an Apple silicon Mac (M1 or later), or `Billy-Jean-<version>-x86_64.dmg` if you have an Intel Mac. Not sure which? Choose **About This Mac** from the Apple menu: "Chip: Apple M…" means Apple silicon, "Processor: Intel…" means Intel.
3. Open the downloaded file and drag **Billy Jean** onto the **Applications** folder.

## Opening it for the first time

Billy Jean is **not code-signed or notarized**, because the project has no Apple Developer account. The first time you open it, macOS may say the app is "damaged" or refuse to open it. The app is fine. You only have to approve it once:

- **macOS 15 (Sequoia) or later:** try to open Billy Jean, then open **System Settings → Privacy & Security**, scroll down to the notice about the blocked app, click **Open Anyway**, and confirm.
- **Older macOS:** right-click (or Control-click) **Billy Jean** in Applications and choose **Open**, then confirm.
- **If neither shows an option** (the warning only offers "Move to Trash"), run this in Terminal, then open the app again:

  ```bash
  xattr -dr com.apple.quarantine "/Applications/Billy Jean.app"
  ```

## Your data

Your sender and bank details are saved on your Mac in `~/Library/Application Support/com.pravinbashyal.billing/identity.json`. Nothing is sent anywhere. Invoices you export are ordinary PDF files, saved where you choose.

## Uninstall

With Homebrew:

```bash
brew uninstall --cask billy-jean
```

This keeps your saved sender and bank details. To remove those too, use `brew uninstall --cask --zap billy-jean`.

Without Homebrew, drag **Billy Jean** from Applications to the Trash, and delete the folder `~/Library/Application Support/com.pravinbashyal.billing` if you want your saved details gone.

## Help and feedback

Found a bug or have an idea? Open an issue at [github.com/pravinbashyal/billy-jean/issues](https://github.com/pravinbashyal/billy-jean/issues).

Want to build it from source or work on it? See [DEVELOPMENT.md](DEVELOPMENT.md).

## License

Billy Jean is released under the MIT License. See [LICENSE](LICENSE). The bundled Noto Sans fonts are licensed under the SIL Open Font License, see `assets/fonts/OFL.txt`.
