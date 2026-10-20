# Record a high-rate CSV

Use `powerz stream` to record the meter's ADC sample stream to a CSV file that
opens in a spreadsheet and matches the vendor recorder's column layout.

1. Pick a rate: `2`, `10`, `50` or `1000` samples per second.
2. Record, either for a fixed time or until Ctrl-C:

   ```bash
   powerz stream --rate 1000 --csv run.csv --duration 60
   powerz stream --rate 50 --csv run.csv          # stop with Ctrl-C
   ```

3. Read the summary on stderr: the sample count, then one line per gap if
   any samples were missed.

The file starts with the header
`ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR`. Time is
counted from the first sample. Numbers are plain decimals with at most 8
significant digits; CHARGE (Ah) and ENERGY (Wh) are integrated from the signed
current, so charging in the reverse direction counts down.

Streaming needs the vendor bulk interface (the default `--transport bulk`);
`cdc` and `hid` carry only `read` and `watch`. To try it without a meter, add
`--transport sim --scenario pd-9v-2a`.
