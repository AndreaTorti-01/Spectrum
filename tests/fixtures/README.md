# Test fixtures

- `edid_qdoled.bin`: 384-byte EDID (base block, CTA-861 extension with HDR static metadata, block map) of a QD-OLED 27 monitor. The manufacturer ID, product code and name were replaced with neutral values (SPC-0001, "QD-OLED 27"); every chromaticity, gamma and HDR byte is the one measured on the real panel. The base-only case is derived in the tests from the first block.
- `hdr_calibration_app.icc`: HDR profile produced by the Windows HDR Calibration app (reference for the MHC2 layout).
- `srgb_inbox.icm`: Windows' inbox `sRGB Color Space Profile.icm`.
