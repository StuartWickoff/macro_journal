#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Audit structurel et de couverture de data_eco/.

Usage : python3 audit.py depuis data_eco/.
Écrit AUDIT.md. Code de sortie :
  1 = défaut structurel, donnée invalide, contrôle externe faux ou journal incohérent ;
  0 = aucune erreur structurelle (des avertissements de couverture peuvent subsister).

Important : les points de contrôle sont des valeurs vérifiées manuellement auprès des
publications citées. Ce script ne télécharge pas les sources et ne prétend pas revalider
en direct chaque observation historique.
"""
import csv
import datetime as dt
import json
import math
import sys
from pathlib import Path

BASE = Path(__file__).resolve().parent
CURS = ["eur", "gbp", "jpy", "chf", "cad", "aud", "nzd", "cny"]
HEADER = ["series_id", "date", "value", "value_secondary"]

MONTHS = [
    f"{y:04d}-{m:02d}"
    for y in range(2023, 2027)
    for m in range(1, 13)
    if (y > 2023 or m >= 11) and (y < 2026 or m <= 10)
]
QUARTERS = [
    f"{y:04d}-{3*q-2:02d}"
    for y in (2023, 2024, 2025, 2026)
    for q in (1, 2, 3, 4)
    if not (y == 2023 and q < 4) and not (y == 2026 and q > 3)
]

EXPECTED = {
    "eur": {"policy_rate", "hicp_headline_yoy", "hicp_core_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "gbp": {"policy_rate", "cpi_headline_yoy", "cpi_core_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "jpy": {"policy_rate", "cpi_headline_yoy", "cpi_core_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "chf": {"policy_rate", "cpi_headline_yoy", "cpi_core_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "cad": {"policy_rate", "cpi_trimmed_yoy", "cpi_median_yoy", "cpi_common",
            "ppi_yoy", "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "aud": {"policy_rate", "cpi_trimmed_yoy", "cpi_weighted_median_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "nzd": {"policy_rate", "cpi_headline_yoy", "cpi_core_proxy_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "sp_global_manufacturing_pmi", "sp_global_services_pmi"},
    "cny": {"policy_rate", "cpi_headline_yoy", "cpi_core_yoy", "ppi_yoy",
            "unemployment_rate", "gdp_qoq", "gdp_yoy",
            "ratingdog_manufacturing_pmi", "ratingdog_services_pmi"},
}

# Absences known to result from the requested source constraint. They are warnings,
# not silently removed from the requested indicator inventory.
UNAVAILABLE = {
    "chf": {
        "sp_global_manufacturing_pmi": "PMI suisse SVME/procure.ch non assimilé à S&P Global ; aucune série S&P vérifiée dans ce dépôt.",
        "sp_global_services_pmi": "Aucune série services suisse S&P Global vérifiée ; aucune substitution.",
    },
    "nzd": {
        "sp_global_manufacturing_pmi": "BNZ-BusinessNZ PMI est une enquête différente ; non substituée à S&P Global.",
        "sp_global_services_pmi": "BNZ-BusinessNZ PSI est une enquête différente ; non substituée à S&P Global.",
    },
}

QUARTERLY = {
    "chf": {"unemployment_rate", "gdp_qoq", "gdp_yoy"},
    "aud": {"cpi_trimmed_yoy", "cpi_weighted_median_yoy", "ppi_yoy", "gdp_qoq", "gdp_yoy"},
    "nzd": {"cpi_headline_yoy", "cpi_core_proxy_yoy", "ppi_yoy", "unemployment_rate",
            "gdp_qoq", "gdp_yoy"},
    **{c: {"gdp_qoq", "gdp_yoy"} for c in ("eur", "gbp", "jpy", "cad", "cny")},
}

CONTROL_POINTS = [
    ("eur", "hicp_headline_yoy", "2026-10-01", 4.0, "Eurostat flash 04/11/2026"),
    ("eur", "hicp_core_yoy", "2026-10-01", 2.5, "Eurostat flash 04/11/2026"),
    ("gbp", "cpi_headline_yoy", "2026-09-01", 3.5, "ONS 21/10/2026"),
    ("gbp", "cpi_core_yoy", "2026-09-01", 2.8, "ONS 21/10/2026"),
    ("cny", "cpi_headline_yoy", "2026-09-01", 0.9, "NBS"),
    ("cny", "cpi_core_yoy", "2026-09-01", 1.0, "NBS"),
    ("cny", "ppi_yoy", "2026-09-01", 5.0, "NBS"),
    ("cny", "unemployment_rate", "2026-09-01", 5.2, "NBS, point contrôlé via miroir"),
    ("cny", "gdp_qoq", "2026-07-01", 1.2, "NBS T3 2026"),
    ("cny", "gdp_yoy", "2026-07-01", 4.5, "NBS T3 2026"),
    ("chf", "cpi_headline_yoy", "2026-10-01", 1.5, "OFS 03/11/2026"),
    ("chf", "cpi_core_yoy", "2026-10-01", 0.5, "OFS 03/11/2026"),
    ("aud", "policy_rate", "2026-10-01", 4.6, "RBA ; décision du 04/11 postérieure à la date"),
    ("cad", "unemployment_rate", "2026-09-01", 6.5, "StatCan LFS 09/10/2026"),
    ("cad", "ppi_yoy", "2026-09-01", 14.5, "Statistics Canada, The Daily 22/10/2026"),
    ("nzd", "unemployment_rate", "2026-07-01", 5.7, "Stats NZ LMS 04/11/2026"),
    ("jpy", "sp_global_services_pmi", "2026-08-01", 52.5, "S&P Global"),
    ("aud", "sp_global_manufacturing_pmi", "2026-10-01", 49.6, "S&P Global 01/11/2026"),
    ("aud", "sp_global_services_pmi", "2026-10-01", 48.5, "S&P Global 03/11/2026"),
    ("cny", "policy_rate", "2026-10-01", 1.4, "PBOC 7-day reverse repo"),
]

def expected_range(series_id):
    if series_id == "policy_rate":
        return (-1.0, 25.0)
    if series_id == "unemployment_rate":
        return (0.0, 30.0)
    if series_id in {"gdp_qoq"}:
        return (-15.0, 15.0)
    if series_id == "gdp_yoy":
        return (-25.0, 25.0)
    if series_id == "ppi_yoy":
        return (-40.0, 60.0)
    if series_id.endswith("_pmi"):
        return (25.0, 75.0)
    if series_id.startswith(("cpi_", "hicp_")) or series_id == "cpi_common":
        return (-10.0, 30.0)
    return None

failures, warnings, report = [], [], []

def fail(message):
    failures.append(message)

def warn(message):
    warnings.append(message)

# Load data and validate the exact CSV contract.
DATA = {}
for currency in CURS:
    path = BASE / currency / "history.csv"
    if not path.exists():
        fail(f"{currency}: fichier history.csv absent")
        DATA[currency] = []
        continue
    with path.open(newline="", encoding="utf-8") as stream:
        reader = csv.reader(stream)
        header = next(reader, [])
        if header != HEADER:
            fail(f"{currency}: en-tête invalide : {header!r}")
            DATA[currency] = []
            continue
        DATA[currency] = list(reader)

total = sum(len(rows) for rows in DATA.values())
series_rows = {}
for currency in CURS:
    seen = set()
    present = {}
    for row_number, row in enumerate(DATA[currency], start=2):
        if len(row) != 4:
            fail(f"{currency}: ligne {row_number}: 4 colonnes attendues, {len(row)} reçues")
            continue
        sid, date_text, value_text, secondary = row
        location = f"{currency}/{sid}/{date_text}/ligne {row_number}"
        if not sid:
            fail(f"{location}: series_id vide")
            continue
        dual_rate = currency == "eur" and sid == "policy_rate"
        if secondary.strip():
            if not dual_rate:
                fail(f"{location}: value_secondary doit rester vide")
        elif dual_rate:
            fail(f"{location}: paire MRR/DFR incomplète : value_secondary vide")
        try:
            observation_date = dt.date.fromisoformat(date_text)
            if observation_date.isoformat() != date_text or observation_date.day != 1:
                raise ValueError
        except ValueError:
            fail(f"{location}: date invalide ou différente du premier jour de période")
            continue
        try:
            value = float(value_text)
            if not math.isfinite(value):
                raise ValueError
        except ValueError:
            fail(f"{location}: valeur non numérique ou non finie ({value_text!r})")
            continue

        if dual_rate:
            try:
                deposit = float(secondary)
                if not math.isfinite(deposit):
                    raise ValueError
            except ValueError:
                fail(f"{location}: value_secondary non numérique ({secondary!r})")
            else:
                if deposit >= value:
                    fail(f"{location}: facilité de dépôt ({secondary}) doit rester inférieure au taux de refinancement ({value_text})")

        key = (sid, date_text)
        if key in seen:
            fail(f"{location}: doublon series_id + date")
        seen.add(key)
        present.setdefault(sid, []).append((date_text, value))

        is_quarterly = sid in QUARTERLY.get(currency, set())
        if is_quarterly:
            if observation_date.month not in (1, 4, 7, 10):
                fail(f"{location}: série trimestrielle datée hors premier mois du trimestre")
            if date_text[:7] not in QUARTERS:
                fail(f"{location}: période trimestrielle hors fenêtre")
        elif date_text[:7] not in MONTHS:
            fail(f"{location}: période mensuelle hors fenêtre")

        bounds = expected_range(sid)
        if bounds and not (bounds[0] <= value <= bounds[1]):
            fail(f"{location}: valeur {value} hors bornes indicatives {bounds}")

    series_rows[currency] = present
    actual = set(present)
    expected = EXPECTED[currency]
    for sid in sorted(expected - actual):
        reason = UNAVAILABLE.get(currency, {}).get(sid)
        if reason:
            warn(f"{currency.upper()}/{sid}: non collecté — {reason}")
        else:
            fail(f"{currency.upper()}: série demandée absente sans cause documentée : {sid}")
    for sid in sorted(actual - expected):
        fail(f"{currency.upper()}: série inattendue : {sid}")

# Independent point checks. Values are fixed from the cited dated publications;
# this script compares the file to the checkpoint, not to a live API.
report += [
    "## Points de contrôle externes",
    "",
    "| Devise | Série | Date | Valeur CSV | Référence | Statut |",
    "|---|---|---|---:|---|---|",
]
for currency, sid, date_text, expected, source in CONTROL_POINTS:
    rows = series_rows.get(currency, {}).get(sid, [])
    match = [value for obs_date, value in rows if obs_date == date_text]
    if not match:
        fail(f"point de contrôle absent : {currency}/{sid}/{date_text}")
        report.append(f"| {currency.upper()} | {sid} | {date_text} | — | {source} | ÉCHEC |")
    elif abs(match[0] - expected) > 0.051:
        fail(f"point de contrôle divergent : {currency}/{sid}/{date_text} = {match[0]}, attendu {expected}")
        report.append(f"| {currency.upper()} | {sid} | {date_text} | {match[0]:g} | {source} | ÉCHEC |")
    else:
        report.append(f"| {currency.upper()} | {sid} | {date_text} | {match[0]:g} | {source} | OK |")

# Coverage table over the full requested windows, including absent-but-requested series.
report += ["", "## Couverture et périodes manquantes", ""]
for currency in CURS:
    report += [f"### {currency.upper()}", "",
               "| Série demandée | Obs. | Première | Dernière | Périodes manquantes |",
               "|---|---:|---|---|---|"]
    for sid in sorted(EXPECTED[currency]):
        observations = sorted(series_rows.get(currency, {}).get(sid, []))
        is_quarterly = sid in QUARTERLY.get(currency, set())
        expected_periods = QUARTERS if is_quarterly else MONTHS
        by_period = {date_text[:7] for date_text, _ in observations}
        missing = [period for period in expected_periods if period not in by_period]
        if is_quarterly:
            missing = [f"{p[:4]}-T{(int(p[5:7])-1)//3+1}" for p in missing]
        first = observations[0][0] if observations else "—"
        last = observations[-1][0] if observations else "—"
        gap = ", ".join(missing) if missing else "—"
        report.append(f"| {sid} | {len(observations)} | {first} | {last} | {gap} |")

        # A small number of latest periods may be unpublished; this threshold is
        # specifically intended to catch structurally incomplete PMI histories
        # and a PPI series represented by a single checkpoint.
        if sid.endswith("_pmi") and len(observations) < 12:
            warn(f"{currency.upper()}/{sid}: historique PMI très incomplet ({len(observations)} observation(s), 36 demandées)")
        if currency == "cad" and sid == "ppi_yoy" and len(observations) < 30:
            warn("CAD/ppi_yoy: seulement 1 point officiel importé ; historique mensuel de 36 mois incomplet")

    report.append("")

# Verify fetch log against all data-bearing series.
report += ["## Cohérence du journal de collecte", ""]
log_path = BASE / "_fetch_log.json"
if not log_path.exists():
    fail("_fetch_log.json absent")
else:
    try:
        log = json.loads(log_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"_fetch_log.json illisible : {exc}")
        log = []
    entries = {}
    for entry in log:
        key = (str(entry.get("currency", "")).lower(), entry.get("series_id"))
        if key in entries:
            fail(f"journal : entrée dupliquée {key}")
        entries[key] = entry
    for currency in CURS:
        for sid, observations in series_rows.get(currency, {}).items():
            entry = entries.get((currency, sid))
            if entry is None:
                fail(f"journal : entrée manquante pour {currency}/{sid}")
                continue
            dates = sorted(date_text for date_text, _ in observations)
            if entry.get("n_obs") != len(observations):
                fail(f"journal {currency}/{sid}: n_obs incohérent")
            if entry.get("first") != dates[0] or entry.get("last") != dates[-1]:
                fail(f"journal {currency}/{sid}: première/dernière date incohérente")
    for currency, sid in entries:
        if currency in CURS and sid not in series_rows.get(currency, {}):
            fail(f"journal : entrée orpheline {currency}/{sid}")
    report.append(f"{len(entries)} entrées dans le journal ; contrôles des comptes et bornes effectués.")

# Report unavailable series separately; this is deliberately not reported as a
# successful data collection.
report += ["", "## Séries demandées mais non collectées", ""]
for currency, unavailable in UNAVAILABLE.items():
    for sid, reason in unavailable.items():
        report.append(f"- {currency.upper()} / {sid} : {reason}")
report.append("")

if warnings:
    report += ["## Avertissements", ""]
    report.extend(f"- {message}" for message in warnings)
    report.append("")

header = [
    "# AUDIT.md — Audit automatisé de data_eco/",
    "",
    f"Exécuté le {dt.date.today().strftime('%d/%m/%Y')}. Script sans dépendance externe.",
    "Les points de contrôle sont des valeurs de référence vérifiées manuellement ;",
    "le script ne télécharge pas les sources et ne revalide pas en direct toutes les observations.",
    "",
    "## Résultat global",
    "",
    f"- **Erreurs structurelles : {len(failures)}**",
    f"- **Avertissements de couverture : {len(warnings)}**",
    f"- **Observations CSV : {total}**",
    "- Une absence documentée et une série historiquement incomplète sont des avertissements,",
    "  pas une validation de complétude.",
    "",
]
if failures:
    header += ["## Erreurs", ""] + [f"- {message}" for message in failures] + [""]
(BASE / "AUDIT.md").write_text("\n".join(header + report) + "\n", encoding="utf-8")
print("\n".join(header[:12]))
print(f"Rapport : {BASE / 'AUDIT.md'}")
sys.exit(1 if failures else 0)
