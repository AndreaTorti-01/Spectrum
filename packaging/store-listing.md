# Microsoft Store submission

Text and answers for Partner Center, section by section. Paste the quoted blocks as they are. Keep this file in step with the app: the certification notes and the description describe real behavior.

## Product identity

Copy these from Partner Center › Product management › Product identity:

- `Package/Identity/Name`, `Package/Identity/Publisher` and `Package/Properties/PublisherDisplayName` go in `$identity` at the top of `packaging/build-msix.ps1`.
- The Store ID goes in `STORE_ID` in `site/src/data/site.ts` and in the README badge link.

## Pricing and availability

- Markets: all.
- Audience: Private audience for 1.0.0 (your account and testers), Public from the next submission.
- Discoverability: make it available and discoverable.
- Schedule: as soon as possible.
- Base price: Free. No free trial, no sale.

## Properties

- Category: Utilities + tools. Subcategory: none. Secondary category: Personalization.
- Personal information: No.
- Privacy policy URL: https://github.com/AndreaTorti-01/Spectrum/blob/main/PRIVACY.md
- Website: https://andreatorti-01.github.io/Spectrum/
- Support contact: https://github.com/AndreaTorti-01/Spectrum/issues
- Product declarations: leave "accessibility tested" unticked. Untick backup of app data to OneDrive (the profiles belong to this PC's monitors).
- System requirements: none.

## Age ratings

IARC questionnaire. Category: app (utility). Answer No to every question: no user-generated content, no chat, no sharing of location or personal data, no purchases, no gambling, no violence.

## Packages

- `Spectrum-X.Y.Z.msix` from the `Spectrum-msix` artifact of the Release run. It is unsigned; the Store signs it.
- Version `X.Y.Z.0`. Device family: Windows 10/11 Desktop only.

## Store listing (English)

Product name: Spectrum: Monitor Color Profiles Manager

Description:

> Your monitor can already report to Windows which colors it can show: Spectrum helps you save it as a color profile. Spectrum also allows you to change your color profile with just one click, right from your system tray. With Spectrum you can easily toggle and tune HDR too. Free and open source.
>
> Read from monitor builds an ICC profile from the primaries and white point in your monitor's EDID. The tray menu lists the profiles of every monitor, and one click makes one the Windows default. Six reference spaces are included: sRGB, Display P3, Adobe RGB, ProPhoto RGB, Rec. 2020 and DCI-P3.
>
> On an HDR display, Spectrum writes an HDR profile in the same format as the Windows HDR Calibration app, with sliders for black level, peak brightness, full-frame brightness and saturation, and sets SDR content brightness. The HDR controls appear only on displays that support HDR.
>
> Compare gamuts draws your monitor's gamut over the CIE 1931 diagram with its coverage of each reference space.
>
> Spectrum changes Windows color profiles and HDR settings only when you choose a profile or move a slider. To put Windows back on its own profiles, open Manage profiles in the main window and click Remove all Spectrum profiles before you uninstall. Profiles you applied otherwise keep working after an uninstall.
>
> Spectrum makes no network connections and collects no data.

Short description:

> Builds ICC and HDR color profiles from your monitor's EDID and switches them from the system tray, one click per monitor.

Product features:

- Builds an ICC profile from the monitor's EDID
- Switches the default color profile from the tray
- Writes HDR profiles in the Windows HDR Calibration app format
- Tunes black level, peak and full-frame brightness and saturation
- Sets SDR content brightness in HDR
- Compares gamuts on the CIE 1931 diagram
- Restores profiles that a driver update dropped

Search terms (7 at most): color profile, ICC, HDR calibration, EDID, monitor, gamut, display

Screenshots: PNG, at least 1366 × 768, four of them: main window (SDR), main window (HDR), gamut window, Manage profiles. Make them with the `screenshots` skill.

App tile icon: `target/store-tile.png` (300 × 300, written by `cargo build`).

Copyright and trademark info: © 2026 Andrea Torti. MIT License.

## Submission options

Publishing hold: "Don't publish this submission until I select Publish now".

Restricted capability `runFullTrust`, justification:

> Spectrum is a Win32 desktop tray app. It needs full trust to write ICC and MHC2 color profiles to the Windows color folder, to set per-user display profile associations through the Windows Color System API (ColorProfileAddDisplayAssociation, ColorProfileSetDisplayDefaultAssociation), and to read and set HDR state and SDR white level through DisplayConfig. These APIs are not available to a partial-trust app.

Notes for certification:

> No account, sign-in or network connection is needed.
> Spectrum starts in the notification area. Click its tray icon to open the main window; right-click it for the profile menu.
> Nothing in Windows changes until the user acts: picking a profile in the dropdown or tray menu, clicking Read from monitor, or moving an HDR slider.
> To undo every change: main window > Manage profiles > Remove all Spectrum profiles (two clicks). This removes Spectrum's profiles and associations so Windows uses its own defaults.
> Works on any monitor. The HDR section appears only on a display that supports HDR, and it is disabled until HDR is turned on.
> Start with Windows is off by default and uses the package's startup task.
