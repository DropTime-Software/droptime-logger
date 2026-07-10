# `.alog` / CSV parser & writer provenance

Clean-room rules (binding, day one): the `.alog` parser and writer in this
module derive **exclusively** from

1. our own documented key map (recorded below as it is built), and
2. self-generated fixture files produced by exporting from a licensed Artisan
   installation ourselves (the hand-authored fixtures in `tests.rs`),

and **never** from Artisan source code. Do not read, copy, or paraphrase
Artisan's Python implementation while working in this directory. Every key we
handle is documented here with how we established its meaning (observation of
self-generated files only).

## File shape

An `.alog` file is a single Python `dict` literal written repr-style on one
line: single-quoted strings, `True`/`False`/`None`, floats, ints. We parse it
with the `py_literal` crate (`ast.literal_eval`-equivalent). We `trim()` the
file before parsing so a trailing newline does not defeat the grammar, and we
tolerate any parse failure by reporting it (never panicking).

Unknown keys are ignored. `extradevices` / `extratimex` / `extratemp1` /
`extratemp2` (auxiliary probe channels) are intentionally ignored in v0.1.0.

## Key map (canonical: °F, seconds)

| key | type (observed) | meaning we assign | handling |
| --- | --- | --- | --- |
| `mode` | `'C'` \| `'F'` | temperature unit of every temp in the file | convert all temps to °F when `'C'`. Absent → auto-detect by BT range (see below). |
| `timex` | `list[float]` | absolute seconds from the start of recording | sample time axis; `timex[0]` is the rebase origin. |
| `temp1` | `list[float]` | ET (environment temp) reading per `timex` index | optional; a value of exactly `-1` is Artisan's "no reading" sentinel → stored as no ET. |
| `temp2` | `list[float]` | **BT** (bean temp) reading per `timex` index — note `temp2` is BT, not `temp1` | required; a value of exactly `-1` is a dropout sentinel → that sample is skipped. |
| `timeindex` | `list[int]` (len 8) | indices into `timex` for `[CHARGE, DRY_END, FC_START, FC_END, SC_START, SC_END, DROP, COOL_END]` | `< 0` → unset for every slot. `== 0` → unset for every slot **except** CHARGE (charge at index 0 is legitimate). Out-of-range indices are ignored. |
| `title` | `str` | roast title | coffee-name fallback (used when `beans` is empty). |
| `beans` | `str` | bean / coffee name | preferred coffee name. |
| `weight` | `[in, out, unit]` | batch weight in/out and its unit | `unit ∈ {g, Kg, lb, oz}` (case-insensitive) → converted to lb; a component of `0` is treated as unset. |
| `roastisodate` | `str` `YYYY-MM-DD` | calendar date of the roast | combined with `roasttime` → `started_wall_ms` (interpreted as UTC; we have no tz in the file). |
| `roasttime` | `str` `HH:MM[:SS]` | wall time of the roast | optional; defaults to `00:00:00` when absent/unparseable. |
| `roastingnotes` | `str` | roasting notes | mapped to notes (joined with `cuppingnotes` when both present). |
| `cuppingnotes` | `str` | cupping notes | appended to notes. |
| `ambientTemp` | `float` | ambient temperature (single scalar) | converted to °F and stored as `ambient_f` on every sample (our schema has no roast-level ambient column). |

`roastdate` (a free-form human string) is intentionally **not** machine-parsed;
`roastisodate` is the machine-readable source of truth. When neither the date
keys nor a parse succeeds, `started_wall_ms` falls back to the file's mtime.

### Unit auto-detection (when `mode` is absent, and for CSV)

Bean temperatures during a roast peak around 390–440 °F but only 195–235 °C, so
the maximum BT across the series cleanly separates the two scales. We treat the
series as Celsius when its maximum BT is `< 250`, otherwise Fahrenheit. `mode`,
when present, always wins over this heuristic.

## `.alog` writer (export)

We emit a dict Artisan can open: `mode` `'F'`, `timex` (= our `session_sec`),
`temp1` (ET, `-1.0` where absent), `temp2` (BT), an 8-slot `timeindex`
(`-1` for every unset marker; the marker's `timex` index otherwise), `title` +
`beans` (coffee name), `weight` `[charge, drop, 'lb']`, `roastisodate` +
`roasttime` (from `started_wall_ms`, UTC), `roastingnotes`, and `ambientTemp`
when a sample carries one. Strings are escaped by `py_literal`'s repr writer;
floats are written in scientific form (valid Python that `literal_eval` reads).

## CSV import

Header-sniffed `time,bt[,et]`. Delimiter is auto-selected among comma,
semicolon and tab (whichever yields the most columns). If the first row is
non-numeric it is treated as a header and columns are matched by name
(`/time/`, `/bt|bean/`, `/et|environ/`, case-insensitive); otherwise columns are
positional `time,bt,et`. `time` is seconds, or `mm:ss` when it contains a colon.
Temperatures are °C/°F auto-detected by the range rule above and converted to
°F. CSV carries no markers; the roast is stored with its raw curve and an
auto-detected turning point. The coffee name defaults to the file stem.

## CSV / JSON export

CSV export writes a `#`-prefixed metadata header (coffee, date, weights,
markers) then `time,bt,et,ror` rows (`time` = seconds from recording start,
`ror` = °F/min from the adjacent BT delta). JSON export is `GetRoastResult`
serialized verbatim with `serde_json` pretty-printing.
