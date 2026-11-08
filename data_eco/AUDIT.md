# AUDIT.md — Audit automatisé de `data_eco/`

Rapport de contrôle actualisé le 08/11/2026 après quatrième passe (paire MRR/DFR du taux directeur EUR + PPI zone euro de septembre) puis cinquième passe (complétion des finales PMI S&P Global eurozone, novembre 2023 → janvier 2025, depuis les archives Investing.com via Wayback Machine croisées avec les communiqués S&P Global/Reuters). Le script `audit.py` a été renforcé ; exécuter `python3 audit.py` depuis `data_eco/` pour régénérer ce rapport. Les points de contrôle sont des références manuelles : le script ne télécharge pas les sources.

## Résultat global

- **Exécution du script le 08/11/2026 : 0 erreur structurelle** (échantillon : 1461 observations)
- Avertissements : 19 (historiques PMI partiels : GBP, JPY, CHF, CAD, AUD ; quatre séries CHF/NZD non collectées ; PPI canadien partiel — les 2 avertissements PMI EUR ont disparu)
- Journal de collecte cohérent : 69 entrées, `n_obs`/`first`/`last` alignés sur les CSV
- Contrôles : structure, schéma (value_secondary vide), dates (1ᵉʳ jour du mois/trimestre), unicité, fenêtre, inventaire des séries, plausibilité, points de contrôle externes, cohérence du journal de collecte.

Total : **1461 observations** sur 8 devises.

## Points de contrôle externes

| Devise | Série | Date | Valeur | Attendu | Référence | Statut |
|---|---|---|---|---|---|---|
| EUR | `hicp_headline_yoy` | 2026-10-01 | 4 | 4 | Eurostat flash 04/11/2026 | OK |
| EUR | `hicp_core_yoy` | 2026-10-01 | 2.5 | 2.5 | Eurostat flash 04/11/2026 | OK |
| GBP | `cpi_headline_yoy` | 2026-09-01 | 3.5 | 3.5 | ONS 21/10/2026 | OK |
| GBP | `cpi_core_yoy` | 2026-09-01 | 2.8 | 2.8 | ONS 21/10/2026 | OK |
| CNY | `cpi_headline_yoy` | 2026-09-01 | 0.9 | 0.9 | NBS | OK |
| CNY | `cpi_core_yoy` | 2026-09-01 | 1 | 1 | NBS | OK |
| CNY | `ppi_yoy` | 2026-09-01 | 5 | 5 | NBS | OK |
| CNY | `unemployment_rate` | 2026-09-01 | 5.2 | 5.2 | NBS | OK |
| CNY | `gdp_qoq` | 2026-07-01 | 1.2 | 1.2 | NBS T3 2026 | OK |
| CNY | `gdp_yoy` | 2026-07-01 | 4.5 | 4.5 | NBS T3 2026 | OK |
| CHF | `cpi_headline_yoy` | 2026-10-01 | 1.5 | 1.5 | OFS 03/11/2026 | OK |
| CHF | `cpi_core_yoy` | 2026-10-01 | 0.5 | 0.5 | OFS 03/11/2026 | OK |
| AUD | `policy_rate` | 2026-10-01 | 4.6 | 4.6 | RBA (4,85 % le 04/11, hors fenêtre) | OK |
| CAD | `unemployment_rate` | 2026-09-01 | 6.5 | 6.5 | StatCan LFS 09/10/2026 | OK |
| CAD | `ppi_yoy` | 2026-09-01 | 14.5 | 14.5 | Statistics Canada, The Daily 22/10/2026 | OK |
| NZD | `unemployment_rate` | 2026-07-01 | 5.7 | 5.7 | Stats NZ LMS 04/11/2026 | OK |
| JPY | `sp_global_services_pmi` | 2026-08-01 | 52.5 | 52.5 | S&P Global (corrigé) | OK |
| AUD | `sp_global_manufacturing_pmi` | 2026-10-01 | 49.6 | 49.6 | S&P Global 01/11/2026 | OK |
| AUD | `sp_global_services_pmi` | 2026-10-01 | 48.5 | 48.5 | S&P Global 03/11/2026 | OK |
| CNY | `policy_rate` | 2026-10-01 | 1.4 | 1.4 | PBOC — inchangé depuis 07/05/2025 | OK |

## Couverture (dates manquantes exactes)

### EUR
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `gdp_qoq` | 12 | 2026-07 | — |
| `gdp_yoy` | 12 | 2026-07 | — |
| `hicp_core_yoy` | 36 | 2026-10 | — |
| `hicp_headline_yoy` | 36 | 2026-10 | — |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 35 | 2026-09 | 2026-10 |
| `sp_global_manufacturing_pmi` | 17 | 2026-10 | 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `sp_global_services_pmi` | 18 | 2026-10 | 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07 |
| `unemployment_rate` | 35 | 2026-09 | 2026-10 |

> `policy_rate` : depuis le 08/11/2026, paire BCE — taux de refinancement MRR (valeur) et facilité de dépôt (secondaire), en vigueur le 1ᵉʳ jour du mois.

### GBP
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_core_yoy` | 35 | 2026-09 | 2026-10 |
| `cpi_headline_yoy` | 35 | 2026-09 | 2026-10 |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 35 | 2026-09 | 2026-10 |
| `sp_global_manufacturing_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `sp_global_services_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `unemployment_rate` | 33 | 2026-07 | 2026-08, 2026-09, 2026-10 |

### JPY
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_core_yoy` | 35 | 2026-09 | 2026-10 |
| `cpi_headline_yoy` | 35 | 2026-09 | 2026-10 |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 35 | 2026-09 | 2026-10 |
| `sp_global_manufacturing_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `sp_global_services_pmi` | 2 | 2026-09 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-10 |
| `unemployment_rate` | 35 | 2026-09 | 2026-10 |

### CHF
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_core_yoy` | 36 | 2026-10 | — |
| `cpi_headline_yoy` | 36 | 2026-10 | — |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 35 | 2026-09 | 2026-10 |
| `unemployment_rate` | 11 | 2026-04 | 2026-T3 |

### CAD
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_common` | 35 | 2026-09 | 2026-10 |
| `cpi_median_yoy` | 35 | 2026-09 | 2026-10 |
| `cpi_trimmed_yoy` | 35 | 2026-09 | 2026-10 |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 1 | 2026-09 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08, 2026-10 | historique partiel : seul septembre 2026 est vérifié et importé |
| `sp_global_manufacturing_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `sp_global_services_pmi` | 3 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07 |
| `unemployment_rate` | 35 | 2026-09 | 2026-10 |

### AUD
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_trimmed_yoy` | 12 | 2026-07 | — |
| `cpi_weighted_median_yoy` | 12 | 2026-07 | — |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 12 | 2026-07 | — |
| `sp_global_manufacturing_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `sp_global_services_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `unemployment_rate` | 35 | 2026-09 | 2026-10 |

### NZD
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_core_proxy_yoy` | 12 | 2026-07 | — |
| `cpi_headline_yoy` | 12 | 2026-07 | — |
| `gdp_qoq` | 11 | 2026-04 | 2026-T3 |
| `gdp_yoy` | 11 | 2026-04 | 2026-T3 |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 11 | 2026-04 | 2026-T3 |
| `unemployment_rate` | 12 | 2026-07 | — |

### CNY
| Série | Obs. | Dernière | Manquant |
|---|---|---|---|
| `cpi_core_yoy` | 35 | 2026-09 | 2026-10 |
| `cpi_headline_yoy` | 35 | 2026-09 | 2026-10 |
| `gdp_qoq` | 12 | 2026-07 | — |
| `gdp_yoy` | 12 | 2026-07 | — |
| `policy_rate` | 36 | 2026-10 | — |
| `ppi_yoy` | 35 | 2026-09 | 2026-10 |
| `ratingdog_manufacturing_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `ratingdog_services_pmi` | 2 | 2026-10 | 2023-11, 2023-12, 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12, 2026-01, 2026-02, 2026-03, 2026-04, 2026-05, 2026-06, 2026-07, 2026-08 |
| `unemployment_rate` | 35 | 2026-09 | 2026-10 |


## Cohérence `_fetch_log.json` ↔ CSV

69 entrées, une par série ; `n_obs` / `first` / `last` cohérents avec les CSV (exécution du 08/11/2026 : 0 écart).
