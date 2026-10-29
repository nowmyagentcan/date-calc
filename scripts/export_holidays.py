"""Write data/holidays.txt from the python-holidays release pinned in requirements.txt.

Format, one block per country, tab-separated:
  V  version  released  first year  last year
  C  code  name  aliases(,)  first year  last year  decree_through or -
  R  region  alias...
  W  * or region  first date  weekend mask Mon..Sun (1 = day off)
  N  name id  name
  H  * or region  YYYYMMDD  name id  [flags: o observed, e estimated]
  X  region  YYYYMMDD  name id        a national holiday the region does not keep
  K  * or region  YYYYMMDD            a weekend day worked by decree
"""

import datetime as dt
import importlib.metadata
import re
import warnings
from pathlib import Path

import holidays
from holidays.registry import COUNTRIES

ROOT = Path(__file__).resolve().parent.parent
FIRST, LAST = 2000, 2050
SUFFIX = re.compile(r" \((observed|estimated|observed, estimated)\)$")
RANGE_WARNING = re.compile(r"available only from (\d{4}) to (\d{4})")
# Letters are bracketed so this file doesn't match its own check.
FLAGGED = re.compile(
    r"\b(cl[a]ude|anthr[o]pic|c[o]dex|op[e]nai|ch[a]tgpt|c[o]pilot|g[e]mini"
    r"|t[o]do|f[i]xme|bl[a]zingly|r[o]bust|s[e]amless|c[o]mprehensive)\b",
    re.IGNORECASE,
)

pin = (ROOT / "scripts/requirements.txt").read_text()
VERSION, RELEASED = re.fullmatch(r"holidays==(\S+)  # released (\S+)\n", pin).groups()
if holidays.__version__ != VERSION:
    raise SystemExit(f"python-holidays {holidays.__version__} is installed; requirements.txt pins {VERSION}")
RELEASE_YEAR = int(RELEASED[:4])

REGISTRY = {names[1]: names for names in COUNTRIES.values()}


def country_name(class_name):
    words = re.findall(r"[A-Z]+(?![a-z])|[A-Z][a-z]*", class_name)
    return " ".join([words[0]] + [w.lower() if w in ("And", "Of", "The") else w for w in words[1:]])


def mask(days):
    return "".join("1" if i in days else "0" for i in range(7))


def entries(h):
    rows = set()
    for date, names in h.items():
        for name in names.split("; "):
            flag = SUFFIX.search(name)
            flags = ("".join(f[0] for f in flag.group(1).split(", ")),) if flag else ()
            rows.add((date.strftime("%Y%m%d"), SUFFIX.sub("", name), flags))
    return rows


def weekends(h, first, last):
    changes, prev, day = [], None, dt.date(first, 1, 1)
    while day.year <= last:
        m = mask(h._get_weekend(day))
        if m != prev:
            changes.append((day.isoformat(), m))
            prev = m
        day += dt.timedelta(1)
    return changes


def decree_through(h):
    years = {d.year for d in h.weekend_workdays}
    for attr in dir(h):
        if attr.startswith("special_") and attr.endswith("_holidays") and isinstance(getattr(h, attr), dict):
            years |= set(getattr(h, attr))
    recent = [y for y in years if RELEASE_YEAR - 4 <= y <= RELEASE_YEAR]
    return str(max(years)) if len(recent) >= 3 else "-"


def block(code):
    cls = holidays.country_holidays(code).__class__
    lang = "en_US" if "en_US" in cls.supported_languages else None
    first, last = max(FIRST, cls.start_year), min(LAST, cls.end_year)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        holidays.country_holidays(code, years=range(first, last + 1), categories=("public",))
    for w in caught:
        a, b = map(int, RANGE_WARNING.search(str(w.message)).groups())
        first, last = max(first, a), min(last, b)

    def load(subdiv=None):
        return holidays.country_holidays(
            code, subdiv=subdiv, years=range(first, last + 1), language=lang, categories=("public",)
        )

    def workdays(h):
        return {d.strftime("%Y%m%d") for d in h.weekend_workdays if first <= d.year <= last}

    nat = load()
    nat_rows, nat_week, nat_work = entries(nat), weekends(nat, first, last), workdays(nat)
    aliases = [a for a in REGISTRY[code][2:] if not a.startswith("Holidays")]
    if any("," in a for a in aliases):
        raise SystemExit(f"{code}: an alias holds a comma: {aliases}")
    lines = [["C", code, country_name(REGISTRY[code][0]), ",".join(aliases), first, last, decree_through(nat)]]
    by_code = {}
    for alias, sub in cls.subdivisions_aliases.items():
        by_code.setdefault(sub, []).append(alias)
    lines += [["R", sub, *by_code.get(sub, [])] for sub in cls.subdivisions]

    regions = []
    for sub in cls.subdivisions:
        h = load(sub)
        rows, week, work = entries(h), weekends(h, first, last), workdays(h)
        if not nat_work <= work:
            raise SystemExit(f"{code}-{sub} drops national decree workdays; the format can't say that")
        regions.append((sub, sorted(rows - nat_rows), sorted(nat_rows - rows), week if week != nat_week else [], sorted(work - nat_work)))

    names = {}
    for sub, add, drop, *_ in [("*", sorted(nat_rows), [], [], [])] + regions:
        for _, name, _ in add + drop:
            names.setdefault(name, len(names))
    lines += [["W", "*", *w] for w in nat_week]
    lines += [["W", r[0], *w] for r in regions for w in r[3]]
    lines += [["N", i, name] for name, i in names.items()]
    lines += [["H", "*", d, names[n], *f] for d, n, f in sorted(nat_rows)]
    lines += [["H", r[0], d, names[n], *f] for r in regions for d, n, f in r[1]]
    lines += [["X", r[0], d, names[n]] for r in regions for d, n, _ in r[2]]
    lines += [["K", "*", d] for d in sorted(nat_work)]
    lines += [["K", r[0], d] for r in regions for d in r[4]]
    return lines


def main():
    warnings.simplefilter("error")
    license_text = importlib.metadata.distribution("holidays").read_text("licenses/LICENSE")
    out = [
        f"# date-calc holiday data. Generated by scripts/export_holidays.py from python-holidays {VERSION} ({RELEASED}), MIT. Do not edit.",
        *(f"# {line}".rstrip() for line in license_text.strip().splitlines()),
        f"V\t{VERSION}\t{RELEASED}\t{FIRST}\t{LAST}",
    ]
    for code in sorted(holidays.list_supported_countries(include_aliases=False)):
        for fields in block(code):
            fields = [str(f) for f in fields]
            if any(re.search(r"[\t\n]", f) for f in fields):
                raise SystemExit(f"{code}: a field holds a separator: {fields}")
            out.append("\t".join(fields))
    text = "\n".join(out) + "\n"
    flagged = [line for line in out if FLAGGED.search(line)]
    if flagged:
        raise SystemExit("These lines would fail the repository's text checks:\n" + "\n".join(flagged))
    (ROOT / "data/holidays.txt").write_bytes(text.encode())
    print(f"data/holidays.txt: {len(out)} lines, {len(text.encode())} bytes")


if __name__ == "__main__":
    main()
