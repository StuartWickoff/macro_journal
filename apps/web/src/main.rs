use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;
use macro_journal_shared::{Entry, EntryDraft};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use web_sys::wasm_bindgen::JsCast;

const API_BASE: &str = "http://127.0.0.1:3000";

#[derive(Clone, Debug, Deserialize)]
struct CotChartAsset {
    currency: String,
    market_name: String,
    chart_file: String,
    latest_date: String,
    is_usd_proxy: bool,
    is_usd_aggregate: bool,
    is_stale: bool,
}

#[derive(Deserialize)]
struct CotManifest {
    markets: Vec<CotChartAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EconomicObservation {
    series_id: String,
    date: String,
    value: f64,
    value_secondary: Option<f64>,
}

/// Une ligne du fichier de contrôle qualité des données (verdicts.csv) :
/// fiabilité de l'historique d'une série, par bloc de dates, avec la
/// preuve. Affichée telle quelle — les corrections restent dans le fichier.
#[derive(Clone, Debug, Deserialize)]
struct DataVerdict {
    currency: String,
    series_id: String,
    block1: String,
    block2: String,
    verdict: String,
    evidence: String,
}

#[derive(Clone, Copy)]
struct CurrencyInfo {
    code: &'static str,
    name: &'static str,
    country: &'static str,
    bank: &'static str,
    flag: &'static str,
    external: bool,
}

const CURRENCIES: [CurrencyInfo; 9] = [
    CurrencyInfo {
        code: "USD",
        name: "Dollar américain",
        country: "États-Unis",
        bank: "Federal Reserve",
        flag: "us",
        external: false,
    },
    CurrencyInfo {
        code: "EUR",
        name: "Euro",
        country: "Zone euro",
        bank: "Banque centrale européenne",
        flag: "eu",
        external: false,
    },
    CurrencyInfo {
        code: "GBP",
        name: "Livre sterling",
        country: "Royaume-Uni",
        bank: "Bank of England",
        flag: "gb",
        external: false,
    },
    CurrencyInfo {
        code: "JPY",
        name: "Yen japonais",
        country: "Japon",
        bank: "Bank of Japan",
        flag: "jp",
        external: false,
    },
    CurrencyInfo {
        code: "CHF",
        name: "Franc suisse",
        country: "Suisse",
        bank: "Swiss National Bank",
        flag: "ch",
        external: false,
    },
    CurrencyInfo {
        code: "CAD",
        name: "Dollar canadien",
        country: "Canada",
        bank: "Bank of Canada",
        flag: "ca",
        external: false,
    },
    CurrencyInfo {
        code: "AUD",
        name: "Dollar australien",
        country: "Australie",
        bank: "Reserve Bank of Australia",
        flag: "au",
        external: false,
    },
    CurrencyInfo {
        code: "NZD",
        name: "Dollar néo-zélandais",
        country: "Nouvelle-Zélande",
        bank: "Reserve Bank of New Zealand",
        flag: "nz",
        external: false,
    },
    CurrencyInfo {
        code: "CNY",
        name: "Yuan renminbi",
        country: "Chine",
        bank: "People’s Bank of China",
        flag: "cn",
        external: true,
    },
];

const UNASSIGNED_CURRENCY: CurrencyInfo = CurrencyInfo {
    code: "UNASSIGNED",
    name: "Notes à classer",
    country: "Anciennes notes",
    bank: "Aucune devise associée",
    flag: "unassigned",
    external: false,
};

fn currency_info(code: &str) -> Option<CurrencyInfo> {
    if code == UNASSIGNED_CURRENCY.code {
        Some(UNASSIGNED_CURRENCY)
    } else {
        CURRENCIES
            .iter()
            .find(|currency| currency.code == code)
            .copied()
    }
}

/// Slug TradingEconomics pour l'ouverture directe des tableaux du pays.
fn tradingeconomics_slug(code: &str) -> &'static str {
    match code {
        "USD" => "united-states",
        "EUR" => "euro-area",
        "GBP" => "united-kingdom",
        "JPY" => "japan",
        "CHF" => "switzerland",
        "CAD" => "canada",
        "AUD" => "australia",
        "NZD" => "new-zealand",
        _ => "china",
    }
}

/// Lien d'une ressource de veille (ouvert dans un nouvel onglet).
#[derive(Clone, Debug, Deserialize)]
struct ResourceLink {
    title: String,
    url: String,
    #[serde(default)]
    note: String,
}

/// Groupe de veille du rituel hebdomadaire (matin, vendredi COT, week-end, BC).
#[derive(Clone, Debug, Deserialize)]
struct ResourceGroup {
    id: String,
    title: String,
    timing: String,
    description: String,
    links: Vec<ResourceLink>,
}

/// Racine de `veille/links.json`.
#[derive(Debug, Deserialize)]
struct VeilleResources {
    groups: Vec<ResourceGroup>,
}

async fn fetch_veille_resources() -> Result<Vec<ResourceGroup>, String> {
    let response = Request::get("/veille/links.json")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    response
        .json::<VeilleResources>()
        .await
        .map(|resources| resources.groups)
        .map_err(|error| error.to_string())
}

/// Série suivies par devise dans le pack hebdo (l'inflation principale est
/// choisie selon la source du pays ; le taux directeur et le chômage font office de socle).
fn pack_series_ids(code: &str) -> &'static [&'static str] {
    match code {
        "USD" => &[
            "fed_target_range",
            "pce_inflation",
            "unemployment_rate",
            "nonfarm_payrolls",
        ],
        "EUR" => &["policy_rate", "hicp_headline_yoy", "unemployment_rate"],
        "CAD" => &["policy_rate", "cpi_trimmed", "unemployment_rate"],
        "AUD" => &["policy_rate", "cpi_trimmed_mean", "unemployment_rate"],
        _ => &["policy_rate", "cpi_headline_yoy", "unemployment_rate"],
    }
}

const PACK_ORDER: [&str; 9] = [
    "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "CNY",
];

/// Deux dernières observations d'une série (valeur + date).
#[derive(Clone, Debug)]
struct RecentSeries {
    series_id: String,
    last: EconomicObservation,
    prev: Option<EconomicObservation>,
}

/// Photo d'une devise au moment de la copie du pack.
#[derive(Clone, Debug, Default)]
struct CurrencySnapshot {
    code: &'static str,
    series: Vec<RecentSeries>,
    journal_note: Option<String>,
}

fn recent_series(observations: &[EconomicObservation], ids: &[&'static str]) -> Vec<RecentSeries> {
    let mut recent = Vec::new();
    for id in ids {
        let mut matched: Vec<EconomicObservation> = observations
            .iter()
            .filter(|observation| observation.series_id == *id)
            .cloned()
            .collect();
        matched.sort_by(|a, b| b.date.cmp(&a.date));
        let last = match matched.first() {
            Some(observation) => observation.clone(),
            None => continue,
        };
        recent.push(RecentSeries {
            series_id: id.to_string(),
            last,
            prev: matched.get(1).cloned(),
        });
    }
    recent
}

/// Dernière note du journal + dernières observations par devise (les
/// indisponibilités API sont ignorées : le pack se construit en mode dégradé).
async fn build_currency_snapshots() -> Vec<CurrencySnapshot> {
    let mut snapshots = Vec::new();
    for code in PACK_ORDER {
        let observations = fetch_economic_observations(code).await.unwrap_or_default();
        let series = recent_series(&observations, pack_series_ids(code));
        let journal_note = fetch_entries(code).await.ok().and_then(|entries| {
            entries
                .first()
                .map(|entry| format!("{} ({})", entry.title, format_date(&entry.updated_at)))
        });
        snapshots.push(CurrencySnapshot {
            code,
            series,
            journal_note,
        });
    }
    snapshots
}

fn cot_lines(cot: Option<&[CotChartAsset]>) -> String {
    match cot {
        Some(markets) if !markets.is_empty() => markets
            .iter()
            .map(|market| {
                format!(
                    "- {} — dernière observation {}{}\n",
                    market.currency,
                    market.latest_date,
                    if market.is_stale {
                        " · observation périmée"
                    } else {
                        ""
                    }
                )
            })
            .collect(),
        _ => "Indisponible (manifest /cot/manifest.json manquant).\n".to_string(),
    }
}

fn compose_weekly_pack(
    cot: Option<&[CotChartAsset]>,
    snapshots: &[CurrencySnapshot],
    groups: &[ResourceGroup],
) -> String {
    let mut pack = String::from(
        "Tu m'aides à préparer ma semaine de trading (méthode de la formation d'analyse fondamentale).\n\
         Pour chaque devise importante de la semaine :\n\
         1) fondamentaux purs — politique monétaire et chiffres majeurs ;\n\
         2) facteurs externes — corrélations, sentiment de risque, géopolitique ;\n\
         3) positionnement actuel — COT et pricing.\n\
         Dégage « à quoi s'attendre » (biais court / moyen / long, ou pas d'avis), puis les opportunités les plus probables de la semaine.\n\n",
    );

    pack.push_str("## Positionnement COT (relevé du vendredi, fenêtre mercredi → mardi)\n");
    pack.push_str(&cot_lines(cot));

    pack.push_str("\n## Données économiques (2 dernières observations)\n");
    if snapshots.is_empty() {
        pack.push_str("Indisponible (API locale hors ligne au moment de la copie).\n");
    }
    for snapshot in snapshots {
        pack.push_str(&format!("\n### {}\n", snapshot.code));
        if snapshot.series.is_empty() {
            pack.push_str("- Aucune donnée (API locale hors ligne ou série vide).\n");
        }
        for series in &snapshot.series {
            let previous = series
                .prev
                .as_ref()
                .map(|observation| format!(" | avant {} ({})", observation.value, observation.date))
                .unwrap_or_default();
            pack.push_str(&format!(
                "- {} : {} ({}){}\n",
                series.series_id, series.last.value, series.last.date, previous
            ));
        }
        if let Some(note) = &snapshot.journal_note {
            pack.push_str(&format!("- Note journal : {}\n", note));
        }
    }

    pack.push_str("\n## Sources à lire (onglet Veille)\n");
    for group in groups {
        for link in &group.links {
            pack.push_str(&format!("- {} — {}\n", link.title, link.url));
        }
    }

    pack.push_str("\n## Cadre de réponse attendu\n");
    pack.push_str(
        "- Par devise : les 3 blocs, puis « à quoi s'attendre » (court / moyen / long).\n",
    );
    pack.push_str("- Maximum 10 lignes par devise. Une surprise hors consensus ET alignée au sens naturel de la devise = setup possible.\n");
    pack
}

fn compose_morning_pack(cot: Option<&[CotChartAsset]>, groups: &[ResourceGroup]) -> String {
    let mut pack = String::from(
        "Tu m'aides à faire ma lecture du matin (méthode de la formation). Maximum 5 à 10 puces, orientées action de la journée.\n\n",
    );
    pack.push_str("1. Sentiment du risque : risk-on / risk-off / neutre — l'indispensable, avant toute autre lecture.\n");
    pack.push_str("2. Calendrier du jour : annonces majeures (taux, inflation, emploi, PMI). Pas d'entrée technique 2 h autour d'une majeure.\n");
    pack.push_str("3. Lectures d'ouverture, dans l'ordre :\n");
    if let Some(morning) = groups.iter().find(|group| group.id == "morning") {
        for link in &morning.links {
            pack.push_str(&format!("   - {} — {}\n", link.title, link.url));
        }
    }
    pack.push_str(
        "\n4. Positionnement COT en vigueur (relevé du vendredi, fenêtre mercredi → mardi) :\n",
    );
    pack.push_str(&cot_lines(cot));
    pack
}

/// Copie via la sélection : sélectionne toute la zone pack puis
/// `document.execCommand("copy")` (chemin le plus large, y compris Firefox).
fn copy_text_to_clipboard() -> bool {
    select_pack_textarea();
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Some(document) = window.document() else {
        return false;
    };
    let Ok(document) = document.dyn_into::<web_sys::HtmlDocument>() else {
        return false;
    };
    document.exec_command("copy").unwrap_or(false)
}

/// Clé de stockage du thème (localStorage, persistant entre les sessions).
const THEME_STORAGE_KEY: &str = "macro_journal::theme";

/// Le visiteur a-t-il mémorisé le thème « papier » ? Nuit par défaut.
fn stored_theme_is_light() -> bool {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(THEME_STORAGE_KEY).ok().flatten())
        .as_deref()
        == Some("light")
}

/// Applique le thème sur le document : attribut `data-theme` sur <html>
/// (le bloc CSS `[data-theme="light"]` reprend les tokens) et `theme-color`
/// de la barre du navigateur aligné.
fn apply_theme(light: bool) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    if let Some(element) = document.document_element() {
        if light {
            let _ = element.set_attribute("data-theme", "light");
        } else {
            let _ = element.remove_attribute("data-theme");
        }
    }
    let color = if light { "#f3f0e8" } else { "#0a111b" };
    if let Ok(Some(meta)) = document.query_selector("meta[name='theme-color']") {
        let _ = meta.set_attribute("content", color);
    }
}

/// Mémorise le choix de thème pour les prochaines visites.
fn persist_theme(light: bool) {
    let value = if light { "light" } else { "dark" };
    if let Some(storage) = web_sys::window()
        .and_then(|window| window.local_storage().ok())
        .flatten()
    {
        let _ = storage.set_item(THEME_STORAGE_KEY, value);
    }
}

/// Sélectionne tout le contenu de la zone pack (copie manuelle Ctrl+C).
fn select_pack_textarea() {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let Some(element) = document.get_element_by_id("pack-contexte") {
                if let Ok(textarea) = element.dyn_into::<web_sys::HtmlTextAreaElement>() {
                    let _ = textarea.focus();
                    textarea.select();
                }
            }
        }
    }
}

/// Série d'inflation principale suivie pour chaque devise : celle que la
/// banque centrale regarde en premier pour juger sa politique.
fn force_inflation_series(code: &str) -> &'static str {
    match code {
        "USD" => "pce_inflation",
        "EUR" => "hicp_headline_yoy",
        "CAD" => "cpi_trimmed",
        "AUD" => "cpi_trimmed_mean",
        _ => "cpi_headline_yoy",
    }
}

fn force_rate_series(code: &str) -> &'static str {
    if code == "USD" {
        "fed_target_range"
    } else {
        "policy_rate"
    }
}

/// Les deux dernières observations d'une série, triées par date.
fn last_two_observations(
    observations: &[EconomicObservation],
    series_id: &str,
) -> (Option<EconomicObservation>, Option<EconomicObservation>) {
    let mut matched: Vec<EconomicObservation> = observations
        .iter()
        .filter(|observation| observation.series_id == series_id)
        .cloned()
        .collect();
    matched.sort_by(|a, b| b.date.cmp(&a.date));
    (matched.first().cloned(), matched.get(1).cloned())
}

/// Valeurs fondamentales d'une devise pour le tableau de lecture :
/// des données brutes, l'interprétation reste celle de l'utilisateur.
#[derive(Clone, Debug)]
struct CurrencyHealth {
    code: &'static str,
    rate: Option<f64>,
    inflation: Option<f64>,
    inflation_prev: Option<f64>,
    unemployment: Option<f64>,
    unemployment_prev: Option<f64>,
    real_rate: Option<f64>,
}

/// Lecture brute d'une devise : taux, inflation et chômage (avec la
/// lecture précédente), taux réel = taux − inflation.
fn currency_health(code: &'static str, observations: &[EconomicObservation]) -> CurrencyHealth {
    let (rate_observation, _) = last_two_observations(observations, force_rate_series(code));
    let (inflation_observation, inflation_previous) =
        last_two_observations(observations, force_inflation_series(code));
    let (unemployment_observation, unemployment_previous) =
        last_two_observations(observations, "unemployment_rate");

    let rate = rate_observation.map(|observation| observation.value);
    let inflation = inflation_observation.map(|observation| observation.value);
    let inflation_prev = inflation_previous.map(|observation| observation.value);
    let unemployment = unemployment_observation.map(|observation| observation.value);
    let unemployment_prev = unemployment_previous.map(|observation| observation.value);
    let real_rate = match (rate, inflation) {
        (Some(rate), Some(inflation)) => Some(rate - inflation),
        _ => None,
    };

    CurrencyHealth {
        code,
        rate,
        inflation,
        inflation_prev,
        unemployment,
        unemployment_prev,
        real_rate,
    }
}

/// Ordre de lecture du tableau : taux réel décroissant, puis ordre
/// alphabétique pour rester stable. Un ordre de lecture seulement —
/// pas un classement mécanique.
fn sort_healths(mut healths: Vec<CurrencyHealth>) -> Vec<CurrencyHealth> {
    healths.sort_by(|a, b| {
        b.real_rate
            .unwrap_or(f64::NEG_INFINITY)
            .partial_cmp(&a.real_rate.unwrap_or(f64::NEG_INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.code.cmp(b.code))
    });
    healths
}

fn pct_with_delta(value: Option<f64>, previous: Option<f64>) -> String {
    match value {
        None => "—".to_string(),
        Some(value) => match previous {
            None => format!("{:.2} %", value),
            Some(previous) => format!("{:.2} % ({:+.2})", value, value - previous),
        },
    }
}

fn currency_flag(code: &str) -> &'static str {
    currency_info(code)
        .map(|currency| currency.flag)
        .unwrap_or("unassigned")
}

/// Fiche d'analyse manuelle : chaque point est une donnée à traiter
/// soi-même, dans l'ordre de la méthode. (identifiant, libellé)
const FICHE_CHECKLIST: [(&str, &[(&str, &str)]); 4] = [
    (
        "Fondamentaux purs",
        &[
            (
                "bc",
                "Banque centrale : décision / minutes / discours traités",
            ),
            (
                "inflation",
                "Inflation lue contre le consensus de la semaine",
            ),
            ("emploi", "Emploi & salaires passés en revue"),
            ("croissance", "Croissance / PMI passés en revue"),
        ],
    ),
    (
        "Facteurs externes",
        &[
            (
                "risque",
                "Sentiment de risque tranché d'abord (risk-on / risk-off)",
            ),
            ("correlations", "Corrélations et matières survolées"),
            ("geopolitique", "Fil géopolitique lu"),
        ],
    ),
    (
        "Positionnement",
        &[
            (
                "cot",
                "COT lu — extrêmes <20 / >80 notés (fenêtre mercredi → mardi)",
            ),
            (
                "pricing",
                "Pricing de la banque centrale lu (attentes intégrées)",
            ),
            ("journal", "Dernière note de ce dossier relue"),
        ],
    ),
    (
        "La semaine",
        &[(
            "surprise",
            "La surprise hors consensus est posée (ou : rien n'a surpris)",
        )],
    ),
];

fn checklist_storage_key(code: &str) -> String {
    format!("macro_journal::devise::{code}::checklist")
}

/// Cases cochées conservées entre les sessions (localStorage du navigateur).
fn load_fiche_checked(code: &str) -> HashSet<String> {
    let Some(window) = web_sys::window() else {
        return HashSet::new();
    };
    let Ok(storage) = window.local_storage() else {
        return HashSet::new();
    };
    let Some(storage) = storage else {
        return HashSet::new();
    };
    let Ok(Some(raw)) = storage.get_item(&checklist_storage_key(code)) else {
        return HashSet::new();
    };
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn save_fiche_checked(code: &str, checked: &HashSet<String>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(storage) = window.local_storage() else {
        return;
    };
    let Some(storage) = storage else {
        return;
    };
    let mut ids: Vec<&str> = checked.iter().map(|id| id.as_str()).collect();
    ids.sort_unstable();
    let raw = serde_json::to_string(&ids).unwrap_or_default();
    let _ = storage.set_item(&checklist_storage_key(code), &raw);
}

/// Markdown de la fiche : les points traités cochés, les autres laissés
/// visibles (ce qui manque doit se voir), puis la conclusion de l'utilisateur.
fn build_fiche_content(code: &str, checked: &HashSet<String>, conclusion: &str) -> String {
    let mut content = format!("# Fiche analyse {}\n\n", code);
    for (group_title, items) in FICHE_CHECKLIST {
        content.push_str(&format!("## {}\n", group_title));
        for (item_id, label) in items {
            if checked.contains(*item_id) {
                content.push_str(&format!("- [x] {}\n", label));
            } else {
                content.push_str(&format!("- [ ] {}\n", label));
            }
        }
        content.push('\n');
    }
    content.push_str("## Ma conclusion\n");
    if conclusion.trim().is_empty() {
        content.push_str("« Pas d'avis cette semaine. »\n");
    } else {
        content.push_str(conclusion.trim());
        content.push('\n');
    }
    content
}

/// Le bilan hebdomadaire : quatre questions du rituel, dans l'ordre du
/// cours. Rien de coché de façon machinale — l'utilisateur écrit.
const BILAN_QUESTIONS: [(&str, &str); 4] = [
    ("attendu", "Ce que vos thèses attendaient"),
    ("surprise", "La surprise de la semaine"),
    ("corriger", "Ce que je corrige dans ma méthode"),
    ("lecon", "La leçon de la semaine"),
];

/// Titre de la note bilan : la devise visée, ou « à classer » quand le
/// bilan porte sur toute la cartographie.
fn bilan_title(code: &str) -> String {
    if code == "UNASSIGNED" {
        "Bilan semaine (à classer)".to_string()
    } else {
        format!("Bilan semaine {code}")
    }
}

/// Markdown du bilan : une section par question ; une section laissée
/// vide reste visible flaguée « Rien d'écrit. » plutôt que supprimée —
/// ce qui manque doit se voir.
fn build_bilan_content(code: &str, answers: &[String]) -> String {
    let mut content = format!("# {}\n\n", bilan_title(code));
    for ((_, label), answer) in BILAN_QUESTIONS.iter().zip(answers.iter()) {
        content.push_str(&format!("## {label}\n"));
        let trimmed = answer.trim();
        if trimmed.is_empty() {
            content.push_str("« Rien d'écrit. »\n");
        } else {
            content.push_str(trimmed);
            content.push('\n');
        }
        content.push('\n');
    }
    content
}

/// Ordre de lecture stable du tableau : devise puis série.
fn sort_verdicts(mut verdicts: Vec<DataVerdict>) -> Vec<DataVerdict> {
    verdicts.sort_by(|a, b| {
        a.currency
            .cmp(&b.currency)
            .then_with(|| a.series_id.cmp(&b.series_id))
    });
    verdicts
}

async fn fetch_verdicts() -> Result<Vec<DataVerdict>, String> {
    let response = Request::get(&format!("{API_BASE}/api/verdicts"))
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }

    response
        .json::<Vec<DataVerdict>>()
        .await
        .map_err(|error| error.to_string())
}

/// Onglet « Bilans » : le rituel du week-end (quatre questions en note,
/// dossier choisi à l'enregistrement) et la fiabilité des données lue
/// dans le fichier de verdicts — corrections dans le fichier, jamais ici.
#[component]
fn ReviewsSection() -> impl IntoView {
    let (verdicts, set_verdicts) = signal::<Option<Result<Vec<DataVerdict>, String>>>(None);
    let (bilan_currency, set_bilan_currency) = signal("UNASSIGNED".to_string());
    let (answers, set_answers) = signal::<Vec<String>>(vec![String::new(); 4]);
    let (bilan_status, set_bilan_status) = signal(String::new());

    spawn_local(async move {
        set_verdicts.set(Some(fetch_verdicts().await));
    });

    let save_bilan = move |_| {
        set_bilan_status.set("ENREGISTREMENT…".to_string());
        let code = bilan_currency.get();
        let answers = answers.get();
        spawn_local(async move {
            let draft = EntryDraft {
                title: bilan_title(&code),
                content: build_bilan_content(&code, &answers),
            };
            match create_entry(&code, draft).await {
                Ok(entry) => set_bilan_status.set(format!(
                    "ENREGISTRÉ DANS LE DOSSIER {code} · {}",
                    format_date(&entry.updated_at)
                )),
                Err(error) => set_bilan_status.set(format!("ÉCHEC DE L'ENREGISTREMENT · {error}")),
            }
        });
    };

    view! {
        <section class="bilan-page">
            <div class="bilan-intro">
                <div class="eyebrow"><span class="eyebrow-dot"></span> "BILANS · RITUEL DE FIN DE SEMAINE"</div>
                <h1>"Comparez vos anticipations aux résultats observés."</h1>
                <p>"Le bilan de fin de semaine : ce que vos thèses attendaient, ce qui les a contrariées, ce que vous corrigez, ce qu'il reste à apprendre. La note est enregistrée dans le dossier de votre choix."</p>
            </div>

            <div class="bilan-form">
                <div class="bilan-row">
                    <label>
                        <span class="eyebrow">"DOSSIER DE LA NOTE"</span>
                        <select prop:value=move || bilan_currency.get() on:change=move |event| set_bilan_currency.set(event_target_value(&event))>
                            <option value="UNASSIGNED">"À classer"</option>
                            {CURRENCIES.iter().map(|currency| view! { <option value={currency.code}>{format!("{} · {}", currency.code, currency.name)}</option> }).collect::<Vec<_>>()}
                        </select>
                    </label>
                </div>

                {BILAN_QUESTIONS.iter().enumerate().map(|(index, (_, question_label))| {
                    view! {
                        <div class="bilan-question">
                            <span class="eyebrow">{*question_label}</span>
                            <textarea
                                rows=3
                                prop:value=move || answers.get().get(index).cloned().unwrap_or_default()
                                on:input=move |event| {
                                    let value = event_target_value(&event);
                                    set_answers.update(|list| {
                                        if let Some(slot) = list.get_mut(index) { *slot = value; }
                                    });
                                }
                            ></textarea>
                        </div>
                    }
                }).collect::<Vec<_>>()}
                <footer class="bilan-foot">
                    <span class="force-status">{move || bilan_status.get()}</span>
                    <button class="button button-primary" on:click=save_bilan>
                        "Enregistrer le bilan"
                    </button>
                </footer>
            </div>

            <div class="bilan-verdicts">
                <header class="bilan-verdicts-head">
                    <div>
                        <div class="eyebrow"><span class="eyebrow-dot"></span> "FIABILITÉ DES DONNÉES"</div>
                        <h2>"Sur quoi repose votre lecture ?"</h2>
                    </div>
                    <p>"Lu dans le fichier de verdicts du pipeline de recherche (route lecture seule). Les corrections se font dans le fichier, jamais ici — c'est votre point E."</p>
                </header>

                {move || match verdicts.get() {
                    None => view! { <p class="force-status">"Lecture du fichier de verdicts…"</p> }.into_any(),
                    Some(Err(error)) => view! {
                        <p class="force-status">{format!("Verdicts indisponibles · {error} — redémarrer le serveur ?")}</p>
                    }.into_any(),
                    Some(Ok(list)) => view! {
                        <div class="bilan-grid">
                            <div class="bilan-grid-head">
                                <span>"DEVISE"</span>
                                <span>"SÉRIE"</span>
                                <span>"BLOC 2023-11 → 2024-12"</span>
                                <span>"BLOC 2025-01 → 2026-10"</span>
                                <span>"VERDICT"</span>
                                <span class="eyebrow">"PREUVE CITÉE"</span>
                            </div>
                            {sort_verdicts(list).into_iter().map(|verdict| {
                                view! {
                                    <div class="bilan-row-item" class:bilan-wrong=verdict.verdict == "WRONG" class:bilan-warn=verdict.verdict == "WARN">
                                        <span><strong>{verdict.currency.clone()}</strong></span>
                                        <span>{verdict.series_id.clone()}</span>
                                        <span>{verdict.block1.clone()}</span>
                                        <span>{verdict.block2.clone()}</span>
                                        <span class="bilan-verdict-badge">{verdict.verdict.clone()}</span>
                                        <small>{verdict.evidence.clone()}</small>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any(),
                }}
            </div>
        </section>
    }
}

/// Onglet « Préparation hebdomadaire » : les liens du rituel de veille,
/// regroupés par moment de la semaine selon la méthode de la formation,
/// plus les packs de contexte prêts à coller dans un chat IA externe.
#[component]
fn WeeklySection(
    cot_manifest: ReadSignal<Option<Result<Vec<CotChartAsset>, String>>>,
) -> impl IntoView {
    let (groups, set_groups) = signal::<Option<Result<Vec<ResourceGroup>, String>>>(None);
    let (pack_text, set_pack_text) = signal::<Option<String>>(None);
    let (pack_kind, set_pack_kind) = signal("PACK HEBDO (SAMEDI)".to_string());
    let (pack_status, set_pack_status) = signal(String::new());

    spawn_local(async move {
        set_groups.set(Some(fetch_veille_resources().await));
    });

    let generate_weekly = move |_| {
        set_pack_status.set("GÉNÉRATION EN COURS…".to_string());
        spawn_local(async move {
            let snapshots = build_currency_snapshots().await;
            let cot = cot_manifest.get().and_then(|result| result.ok());
            let sources = groups
                .get()
                .and_then(|result| result.ok())
                .unwrap_or_default();
            let pack = compose_weekly_pack(cot.as_deref(), &snapshots, &sources);
            set_pack_kind.set("PACK HEBDO (SAMEDI)".to_string());
            set_pack_text.set(Some(pack));
            set_pack_status.set("PACK GÉNÉRÉ — VÉRIFIEZ, ÉDITEZ, PUIS COPIEZ".to_string());
        });
    };

    let generate_morning = move |_| {
        set_pack_status.set("GÉNÉRATION EN COURS…".to_string());
        spawn_local(async move {
            let cot = cot_manifest.get().and_then(|result| result.ok());
            let sources = groups
                .get()
                .and_then(|result| result.ok())
                .unwrap_or_default();
            let pack = compose_morning_pack(cot.as_deref(), &sources);
            set_pack_kind.set("BRIEF DU MATIN".to_string());
            set_pack_text.set(Some(pack));
            set_pack_status.set("BRIEF GÉNÉRÉ — VÉRIFIEZ, ÉDITEZ, PUIS COPIEZ".to_string());
        });
    };

    let copy_pack = move |_| {
        if pack_text.get().is_some() {
            let copied = copy_text_to_clipboard();
            set_pack_status.set(if copied {
                "COPIÉ".to_string()
            } else {
                "COPIE MANUELLE : SÉLECTIONNEZ TOUT (CTRL+C)".to_string()
            });
        }
    };

    let close_pack = move |_| {
        set_pack_text.set(None);
        set_pack_status.set(String::new());
    };

    view! {
        <section class="veille-page">
            <div class="veille-intro">
                <div class="eyebrow"><span class="eyebrow-dot"></span> "PRÉPARATION HEBDOMADAIRE · VEILLE"</div>
                <h1>"Préparez la semaine avec les données publiées."</h1>
                <p>"Vendredi : relevé COT. Samedi — dimanche : un résumé par devise importante, puis une attente à court / moyen / long terme. Chaque matin vers 7 h : sentiment de risque, puis lectures. Tout est à un clic ci-dessous."</p>
            </div>

            <div class="pack-strip">
                <div class="pack-strip-copy">
                    <div class="eyebrow">"PACKS DE CONTEXTE"</div>
                    <h2>"Des packs de contexte prêts à l'emploi."</h2>
                    <p>"La méthode du cours, enrichie de vos dernières données publiées (COT, taux, inflation, chômage, dernière note du journal). L'analyse générative reste dans votre chat : le tableau de bord fournit le contexte."</p>
                </div>
                <div class="pack-strip-actions">
                    <button class="button button-quiet" on:click=generate_morning>"Brief du matin"</button>
                    <button class="button button-primary" on:click=generate_weekly>"Pack hebdo du samedi"</button>
                </div>
                <span class="pack-status">{move || pack_status.get()}</span>
            </div>

            {move || match groups.get() {
                None => view! {
                    <p class="veille-status">"Chargement des liens…"</p>
                }
                .into_any(),
                Some(Err(error)) => view! {
                    <p class="veille-error">{format!(
                        "Fichier de veille indisponible ({error}). Vérifiez apps/web/public/veille/links.json."
                    )}</p>
                }
                .into_any(),
                Some(Ok(items)) if items.is_empty() => view! {
                    <p class="veille-error">"Aucun groupe de liens disponible — vérifiez veille/links.json."</p>
                }
                .into_any(),
                Some(Ok(items)) => view! {
                    <div class="veille-grid">
                        <For
                            each=move || items.clone()
                            key=|group| group.id.clone()
                            children=move |group| view! { <ResourceGroupCard group/> }
                        />
                    </div>
                }
                .into_any(),
            }}

            <Show when=move || pack_text.get().is_some()>
                <div class="pack-overlay">
                    <div class="pack-modal">
                        <header class="pack-modal-head">
                            <span class="eyebrow"><span class="eyebrow-dot"></span> {move || pack_kind.get()}</span>
                            <h2>"Pack de contexte"</h2>
                            <p>"Collez ce texte dans votre chat IA habituel, générez la synthèse, puis conservez-la dans une note du dossier devise correspondant. Le texte reste éditable avant la copie."</p>
                        </header>
                        <textarea
                            id="pack-contexte"
                            class="pack-textarea"
                            prop:value=move || pack_text.get().unwrap_or_default()
                            on:input=move |event| set_pack_text.set(Some(event_target_value(&event)))
                        ></textarea>
                        <footer class="pack-modal-foot">
                            <span class="pack-status">{move || pack_status.get()}</span>
                            <button class="button button-quiet" on:click=move |_| select_pack_textarea()>"Sélectionner tout"</button>
                            <button class="button button-quiet" on:click=close_pack>"Fermer"</button>
                            <button class="button button-primary" on:click=copy_pack>"Copier"</button>
                        </footer>
                    </div>
                </div>
            </Show>
        </section>
    }
}

/// Onglet « Analyse des paires » : les fondamentaux des 9 devises en
/// valeurs brutes (l'interprétation est manuelle), et une fiche par
/// devise — checklist de données traitées + conclusion utilisateur,
/// enregistrable comme note dans le dossier de la devise.
#[component]
fn PairsSection() -> impl IntoView {
    let (healths, set_healths) = signal::<Option<Vec<CurrencyHealth>>>(None);
    let (fiche_currency, set_fiche_currency) = signal::<Option<&'static str>>(None);
    let (fiche_checked, set_fiche_checked) = signal::<HashSet<String>>(HashSet::new());
    let (fiche_conclusion, set_fiche_conclusion) = signal(String::new());
    let (fiche_status, set_fiche_status) = signal(String::new());

    spawn_local(async move {
        let mut result = Vec::new();
        for currency in CURRENCIES {
            let observations = fetch_economic_observations(currency.code)
                .await
                .unwrap_or_default();
            result.push(currency_health(currency.code, &observations));
        }
        set_healths.set(Some(result));
    });

    let open_fiche = move |code: &'static str| {
        set_fiche_conclusion.set(String::new());
        set_fiche_status.set(String::new());
        set_fiche_currency.set(Some(code));
        set_fiche_checked.set(load_fiche_checked(code));
    };

    let close_fiche = move |_| set_fiche_currency.set(None);

    let toggle_fiche_item = move |item_id: &'static str, checked_now: bool| {
        set_fiche_checked.update(|set| {
            if checked_now {
                set.insert(item_id.to_string());
            } else {
                set.remove(item_id);
            }
        });
        if let Some(code) = fiche_currency.get() {
            save_fiche_checked(code, &fiche_checked.get());
        }
    };

    let save_fiche = move |_| {
        let Some(code) = fiche_currency.get() else {
            return;
        };
        set_fiche_status.set("ENREGISTREMENT…".to_string());
        spawn_local(async move {
            let draft = EntryDraft {
                title: format!("Fiche analyse {code}"),
                content: build_fiche_content(code, &fiche_checked.get(), &fiche_conclusion.get()),
            };
            match create_entry(code, draft).await {
                Ok(entry) => set_fiche_status.set(format!(
                    "ENREGISTRÉ DANS LE DOSSIER {code} · {}",
                    format_date(&entry.updated_at)
                )),
                Err(error) => set_fiche_status.set(format!("ÉCHEC DE L'ENREGISTREMENT · {error}")),
            }
        });
    };

    view! {
        <section class="force-page">
            <div class="force-intro">
                <div class="eyebrow"><span class="eyebrow-dot"></span> "ANALYSE DES PAIRES · FONDAMENTAUX"</div>
                <h1>"Comparez la force relative des devises."</h1>
                <p>"Les chiffres bruts de chaque devise, triés par taux réel — l'appréciation reste la vôtre. Cliquez une ligne : la fiche d'analyse s'ouvre. Vous cochez ce que vous avez traité, vous écrivez votre conclusion, elle part dans le dossier de la devise."</p>
            </div>

            {move || match healths.get() {
                None => view! {
                    <p class="force-status">"Lecture des données économiques des 9 devises…"</p>
                }
                .into_any(),
                Some(_) => {
                    view! {
                        <div class="force-table">
                            <div class="force-table-head">
                                <span>"DEVISE"</span>
                                <span>"TAUX"</span>
                                <span>"INFLATION"</span>
                                <span>"CHÔMAGE"</span>
                                <span>"TAUX RÉEL"</span>
                            </div>
                            <For
                                each=move || sort_healths(healths.get().unwrap_or_default())
                                key=|health| health.code
                                children=move |health| {
                                    view! {
                                        <button class="force-row" on:click=move |_| open_fiche(health.code)>
                                            <span class="force-code">
                                                <img src=format!("/flags/{}.png", currency_flag(health.code)) alt=health.code/>
                                                <strong>{health.code}</strong>
                                            </span>
                                            <span>{pct_with_delta(health.rate, None)}</span>
                                            <span>{pct_with_delta(health.inflation, health.inflation_prev)}</span>
                                            <span>{pct_with_delta(health.unemployment, health.unemployment_prev)}</span>
                                            <span>{pct_with_delta(health.real_rate, None)}</span>
                                        </button>
                                    }
                                }
                            />
                        </div>

                        <p class="force-note">"Inflation : série principale suivie par chaque banque centrale (PCE côté US, HICP côté euro, trimmed/trimmed-mean ailleurs). Les Δ entre parenthèses valent la variation vs la lecture précédente. Le tri par taux réel n'est qu'un ordre delecture — pas un verdict."</p>
                    }
                    .into_any()
                }
            }}

            {move || fiche_currency.get().map(|code| {
                let name = currency_info(code).map(|currency| currency.name).unwrap_or(code);
                view! {
                    <div class="fiche">
                        <header class="fiche-head">
                            <span class="force-code">
                                <img src=format!("/flags/{}.png", currency_flag(code)) alt=code/>
                                <strong>{name}</strong>
                                <span class="fiche-code">{code}</span>
                            </span>
                            <button class="button button-quiet" on:click=close_fiche>"Fermer"</button>
                        </header>

                        <p class="fiche-rule">"Cochez seulement ce que vous avez réellement traité cette semaine. Ce qui reste décoché dit ce qu'il manque. La conclusion est à vous."</p>

                        <For
                            each=move || {
                                FICHE_CHECKLIST.iter().collect::<Vec<_>>()
                            }
                            key=|group| group.0
                            children=move |group| {
                                view! {
                                    <div class="fiche-group">
                                        <span class="eyebrow">{group.0}</span>
                                        <div class="fiche-items">
                                            {group.1.iter().map(|item| {
                                                let item_id = item.0;
                                                let label = item.1;
                                                view! {
                                                    <label class="fiche-item">
                                                        <input
                                                            type="checkbox"
                                                            checked=move || fiche_checked.get().contains(item_id)
                                                            on:change=move |event| {
                                                                toggle_fiche_item(item_id, event_target_checked(&event));
                                                            }
                                                        />
                                                        <span>{label}</span>
                                                    </label>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    </div>
                                }
                            }
                        />

                        <label class="fiche-conclusion">
                            <span class="eyebrow">"MA CONCLUSION · À QUOI S'ATTENDRE"</span>
                            <textarea
                                id="fiche-conclusion"
                                prop:value=move || fiche_conclusion.get()
                                on:input=move |event| set_fiche_conclusion.set(event_target_value(&event))
                                placeholder="Votre scénario aux horizons court, moyen ou long terme."
                            ></textarea>
                        </label>

                        <footer class="fiche-foot">
                            <span class="force-status">{move || fiche_status.get()}</span>
                            <button class="button button-primary" on:click=save_fiche>
                                "Enregistrer dans le dossier"
                            </button>
                        </footer>
                    </div>
                }
                .into_any()
            })}
        </section>
    }
}

#[component]
fn ResourceGroupCard(group: ResourceGroup) -> impl IntoView {
    let links = group.links.clone();
    view! {
        <article class="veille-card">
            <header class="veille-card-head">
                <span class="veille-timing">{group.timing.clone()}</span>
                <h2>{group.title.clone()}</h2>
                <p>{group.description.clone()}</p>
            </header>
            <ul class="veille-links">
                <For
                    each=move || links.clone()
                    key=|link| link.url.clone()
                    children=move |link| view! {
                        <li>
                            <a href=link.url.clone() target="_blank" rel="noopener noreferrer">
                                <span class="veille-link-title">{link.title.clone()}</span>
                                <span class="veille-link-note">{link.note.clone()}</span>
                            </a>
                        </li>
                    }
                />
            </ul>
        </article>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let (active_section, set_active_section) = signal("currencies".to_string());
    let (entries, set_entries) = signal::<Vec<Entry>>(Vec::new());
    let (active_id, set_active_id) = signal::<Option<i64>>(None);

    // Thème clair/sombre : appliqué une fois au montage, bascule depuis la
    // topbar, choix mémorisé dans localStorage.
    let (light_theme, set_light_theme) = signal(stored_theme_is_light());
    apply_theme(light_theme.get());
    let toggle_theme = move |_| {
        set_light_theme.update(|light| {
            *light = !*light;
            apply_theme(*light);
            persist_theme(*light);
        });
    };
    let (title, set_title) = signal(String::new());
    let (content, set_content) = signal(String::new());
    let (search, set_search) = signal(String::new());
    let (selected_currency, set_selected_currency) = signal::<Option<String>>(None);
    let (editor_open, set_editor_open) = signal(false);
    let (editing, set_editing) = signal(false);
    let (preview_mode, set_preview_mode) = signal(false);
    let (save_status, set_save_status) = signal("BASE LOCALE · CONNEXION".to_string());
    let (api_status, set_api_status) = signal(false);
    let (sidebar_visible, set_sidebar_visible) = signal(true);
    let (cot_manifest, set_cot_manifest) =
        signal::<Option<Result<Vec<CotChartAsset>, String>>>(None);
    let (currency_filter, set_currency_filter) = signal::<HashSet<String>>(HashSet::new());

    let load_entries = move |currency_code: String| {
        spawn_local(async move {
            let still_selected = selected_currency.get().as_deref() == Some(currency_code.as_str())
                && active_section.get() == "currencies";
            match fetch_entries(&currency_code).await {
                Ok(items) if still_selected => {
                    set_entries.set(items);
                    set_save_status.set("BASE LOCALE · CONNECTÉE".to_string());
                    set_api_status.set(true);
                }
                Err(error) if still_selected => {
                    set_save_status.set(format!("API INDISPONIBLE · {error}"));
                    set_api_status.set(false);
                }
                _ => {}
            }
        });
    };

    spawn_local(async move {
        match check_api().await {
            Ok(()) => {
                set_api_status.set(true);
                set_save_status.set("BASE LOCALE · CONNECTÉE".to_string());
            }
            Err(error) => {
                set_api_status.set(false);
                set_save_status.set(format!("API INDISPONIBLE · {error}"));
            }
        }
    });

    spawn_local(async move {
        set_cot_manifest.set(Some(fetch_cot_manifest().await));
    });

    let new_note = move |_| {
        set_active_id.set(None);
        set_title.set(String::new());
        set_content.set(String::new());
        set_editor_open.set(true);
        set_editing.set(true);
        set_preview_mode.set(false);
        set_save_status.set("NOUVEAU BROUILLON · NON ENREGISTRÉ".to_string());
    };

    // Suppression définitive de la note ouverte : confirmation, DELETE
    // serveur, puis retour à la liste et rechargement de la vue courante.
    let remove_note = move |_| {
        let Some(id) = active_id.get() else {
            return;
        };
        let confirmed = web_sys::window()
            .and_then(|window| {
                window
                    .confirm_with_message("Supprimer définitivement cette note ?")
                    .ok()
            })
            .unwrap_or(false);
        if !confirmed {
            return;
        }
        let currency = selected_currency.get().unwrap_or_default();
        let still = active_section.get().clone();
        spawn_local(async move {
            match delete_entry_request(id).await {
                Ok(()) => {
                    set_editor_open.set(false);
                    set_active_id.set(None);
                    set_title.set(String::new());
                    set_content.set(String::new());
                    set_save_status.set("NOTE SUPPRIMÉE".to_string());
                    if currency.is_empty() {
                        if let Ok(items) = fetch_all_entries().await {
                            if still == "journal" {
                                set_entries.set(items);
                            }
                        }
                    } else {
                        load_entries(currency);
                    }
                }
                Err(error) => {
                    set_save_status.set(format!("SUPPRESSION IMPOSSIBLE · {error}"));
                }
            }
        });
    };

    let open_currency = move |code: &'static str| {
        set_active_section.set("currencies".to_string());
        set_selected_currency.set(Some(code.to_string()));
        set_editor_open.set(false);
        set_search.set(String::new());
        set_currency_filter.set(HashSet::new());
        set_entries.set(Vec::new());
        load_entries(code.to_string());
    };

    let open_unassigned = move |_| {
        set_active_section.set("journal".to_string());
        set_selected_currency.set(None);
        set_editor_open.set(false);
        set_search.set(String::new());
        set_currency_filter.set(HashSet::new());
        set_entries.set(Vec::new());
        spawn_local(async move {
            match fetch_all_entries().await {
                Ok(items) => {
                    if active_section.get() == "journal" {
                        set_entries.set(items);
                        set_save_status.set("BASE LOCALE · CONNECTÉE".to_string());
                        set_api_status.set(true);
                    }
                }
                Err(error) => {
                    if active_section.get() == "journal" {
                        set_save_status.set(format!("API INDISPONIBLE · {error}"));
                        set_api_status.set(false);
                    }
                }
            }
        });
    };

    let toggle_currency_filter = move |code: &'static str| {
        set_currency_filter.update(|selected| {
            if selected.contains(code) {
                selected.remove(code);
            } else {
                selected.insert(code.to_string());
            }
        });
    };
    let clear_currency_filter = move |_| {
        set_currency_filter.set(HashSet::new());
    };

    let save_note = move |_| {
        let draft = EntryDraft {
            title: title.get().trim().to_string(),
            content: content.get(),
        };
        let existing_id = active_id.get();
        let currency_code = selected_currency
            .get()
            .unwrap_or_else(|| "UNASSIGNED".to_string());

        if draft.title.is_empty() {
            set_save_status.set("UN TITRE EST REQUIS".to_string());
            return;
        }

        set_save_status.set("ENREGISTREMENT…".to_string());

        spawn_local(async move {
            let result = match existing_id {
                Some(id) => update_entry(id, &currency_code, draft).await,
                None => create_entry(&currency_code, draft).await,
            };

            match result {
                Ok(saved) => {
                    set_active_id.set(Some(saved.id));
                    set_entries.update(|all| {
                        if let Some(existing) = all.iter_mut().find(|item| item.id == saved.id) {
                            *existing = saved.clone();
                        } else {
                            all.insert(0, saved.clone());
                        }
                        all.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
                    });
                    set_save_status.set(format!(
                        "ENREGISTRÉ LOCALEMENT · {}",
                        format_date(&saved.updated_at)
                    ));
                    set_api_status.set(true);
                    set_editor_open.set(false);
                    set_editing.set(false);
                }
                Err(error) => {
                    set_save_status.set(format!("ÉCHEC DE L’ENREGISTREMENT · {error}"));
                    set_api_status.set(false);
                }
            }
        });
    };

    let preview_html = move || render_markdown(&content.get());
    let character_count = move || content.get().chars().count();
    let filtered_entries =
        move || entries_matching_filter(&entries.get(), &currency_filter.get(), &search.get());
    let section_title = move || match active_section.get().as_str() {
        "journal" => "Notes à classer",
        "currencies" => selected_currency
            .get()
            .and_then(|code| currency_info(&code))
            .map(|currency| currency.code)
            .unwrap_or("Dossiers de devises"),
        "weekly" => "Préparation hebdomadaire",
        "pairs" => "Analyse des paires",
        "reviews" => "Bilans",
        _ => "Journal de recherche",
    };

    view! {
        <div class="app-shell" class:sidebar_hidden=move || !sidebar_visible.get()>
            <aside class="sidebar">
                <a class="brand-lockup" href="#" on:click=open_unassigned>
                    <div class="brand-mark">"M"</div>
                    <div class="brand-copy">
                        <strong>"MACRO JOURNAL"</strong>
                        <span>"CARNET DE RECHERCHE"</span>
                    </div>
                </a>

                <div class="sidebar-section-label">"ESPACE DE TRAVAIL"</div>
                <nav class="main-nav" aria-label="Navigation principale">
                    <button class="nav-item" class:active=move || active_section.get() == "journal" on:click=open_unassigned>
                        <span class="nav-icon">"▤"</span><span>"Notes à classer"</span>
                    </button>
                    <button class="nav-item" class:active=move || active_section.get() == "currencies" on:click=move |_| {
                        set_active_section.set("currencies".to_string());
                        set_selected_currency.set(None);
                        set_editor_open.set(false);
                    }>
                        <span class="nav-icon">"◎"</span><span>"Devises"</span>
                    </button>
                    <button class="nav-item" class:active=move || active_section.get() == "weekly" on:click=move |_| set_active_section.set("weekly".to_string())>
                        <span class="nav-icon">"▦"</span><span>"Préparation hebdomadaire"</span>
                    </button>
                    <button class="nav-item" class:active=move || active_section.get() == "pairs" on:click=move |_| set_active_section.set("pairs".to_string())>
                        <span class="nav-icon">"⇄"</span><span>"Analyse des paires"</span>
                    </button>
                    <button class="nav-item" class:active=move || active_section.get() == "reviews" on:click=move |_| set_active_section.set("reviews".to_string())>
                        <span class="nav-icon">"↺"</span><span>"Bilans"</span>
                    </button>
                </nav>

                <div class="sidebar-spacer"></div>
                <div class="sidebar-note">
                    <span class="sidebar-note-mark">"“"</span>
                    <p><span>"Les faits d’abord."</span><span>"La conviction ensuite."</span></p>
                    <span>"VOTRE MÉTHODE, VOTRE HISTORIQUE"</span>
                </div>
                <div class="sidebar-bottom">
                    <div class="connection-row">
                        <span class="connection-dot" class:online=move || api_status.get()></span>
                        <span>{move || if api_status.get() { "BASE LOCALE CONNECTÉE" } else { "BASE LOCALE HORS LIGNE" }}</span>
                    </div>
                    <div class="sidebar-version">"MACRO JOURNAL · 0.1.0"</div>
                </div>
            </aside>

            <main class="main-area">
                <header class="topbar">
                    <div class="topbar-start">
                        <button
                            type="button"
                            class="sidebar-toggle"
                            aria-label=move || if sidebar_visible.get() { "Masquer la barre latérale" } else { "Afficher la barre latérale" }
                            aria-expanded=move || sidebar_visible.get().to_string()
                            title=move || if sidebar_visible.get() { "Masquer la navigation" } else { "Afficher la navigation" }
                            on:click=move |_| set_sidebar_visible.update(|visible| *visible = !*visible)
                        >
                            <span aria-hidden="true">{move || if sidebar_visible.get() { "‹" } else { "☰" }}</span>
                        </button>
                        <div class="breadcrumb">
                            <span>"ESPACE DE TRAVAIL"</span><span class="breadcrumb-separator">"/"</span>
                            <strong>{section_title}</strong>
                        </div>
                    </div>
                    <div class="topbar-actions">
                        <button
                            type="button"
                            class="theme-toggle"
                            aria-label="Basculer entre le thème sombre et le thème clair"
                            aria-pressed=move || light_theme.get().to_string()
                            title=move || if light_theme.get() { "Passer au thème sombre" } else { "Passer au thème clair" }
                            on:click=toggle_theme
                        >
                            <span aria-hidden="true">{move || if light_theme.get() { "☾" } else { "☀" }}</span>
                        </button>
                        <span class="topbar-private"><span>"●"</span> "LOCAL & PRIVÉ"</span>
                        <div class="avatar">"MJ"</div>
                    </div>
                </header>

                {move || {
                    if editor_open.get()
                        && (active_section.get() == "journal"
                            || selected_currency.get().is_some())
                    {
                        view! {
                            <section class="editor-page">
                                <div class="editor-page-top">
                                    <button class="back-link" on:click=move |_| set_editor_open.set(false)>
                                        <span aria-hidden="true">"←"</span>
                                        {move || selected_currency.get()
                                            .and_then(|code| currency_info(&code))
                                            .map(|currency| format!("Retour au dossier {}", currency.code))
                                            .unwrap_or_else(|| "Retour aux notes à classer".to_string())}
                                    </button>
                                    <div class="editor-actions">
                                        <span class="save-state">{save_status}</span>
                                        <Show when=move || !editing.get()>
                                            <button class="button button-quiet" on:click=move |_| set_editing.set(true)>
                                                <span>"✎"</span> "Modifier"
                                            </button>
                                        </Show>
                                        <Show when=move || editing.get()>
                                            <button
                                                class="button button-quiet"
                                                class:button-selected=move || preview_mode.get()
                                                on:click=move |_| set_preview_mode.update(|preview| *preview = !*preview)
                                            >
                                                {move || if preview_mode.get() { "Reprendre l’écriture" } else { "Aperçu" }}
                                            </button>
                                            <button class="button button-primary" on:click=save_note>
                                                <span>"✓"</span> "Enregistrer"
                                            </button>
                                        </Show>
                                        <Show when=move || active_id.get().is_some()>
                                            <button class="button button-danger" on:click=remove_note>
                                                <span aria-hidden="true">"✕"</span> "Supprimer"
                                            </button>
                                        </Show>
                                    </div>
                                </div>

                                <article class="document-card">
                                    <div class="document-kicker">
                                        {move || selected_currency.get()
                                            .and_then(|code| currency_info(&code))
                                            .map(|currency| view! {
                                                <img class="document-flag" src=format!("/flags/{}.png", currency.flag) alt=format!("Drapeau : {}", currency.country)/>
                                            }.into_any())
                                            .unwrap_or_else(|| view! { <span class="document-kicker-mark">"M"</span> }.into_any())}
                                        <span>{move || selected_currency.get()
                                            .and_then(|code| currency_info(&code))
                                            .map(|currency| format!("DOSSIER {}", currency.code))
                                            .unwrap_or_else(|| "NOTES À CLASSER".to_string())}</span>
                                        <span class="kicker-divider">"·"</span>
                                        <span>{move || if active_id.get().is_some() { "NOTE" } else { "NOUVELLE NOTE" }}</span>
                                    </div>

                                    <Show
                                        when=move || editing.get()
                                        fallback=move || view! {
                                            <h1 class="document-title">{move || title.get()}</h1>
                                        }
                                    >
                                        <input
                                            class="document-title-input"
                                            type="text"
                                            aria-label="Titre de l’analyse"
                                            placeholder="Titre de l’observation"
                                            prop:value=move || title.get()
                                            on:input=move |event| {
                                                set_title.set(event_target_value(&event));
                                                set_save_status.set("MODIFICATIONS NON ENREGISTRÉES".to_string());
                                            }
                                        />
                                    </Show>

                                    <div class="document-byline">
                                        <span class="document-status-dot"></span>
                                        <span>{move || if active_id.get().is_some() { "RECHERCHE PERSONNELLE" } else { "BROUILLON" }}</span>
                                        <span class="kicker-divider">"·"</span>
                                        <span>{move || {
                                            entries.get().iter()
                                                .find(|entry| Some(entry.id) == active_id.get())
                                                .map(|entry| format_date(&entry.updated_at))
                                                .unwrap_or_else(|| "À l’instant".to_string())
                                        }}</span>
                                    </div>

                                    <div class="document-rule"></div>
                                    <Show
                                        when=move || editing.get() && !preview_mode.get()
                                        fallback=move || view! {
                                            <Show
                                                when=move || !content.get().trim().is_empty()
                                                fallback=|| view! {
                                                    <div class="document-empty">
                                                        <div class="empty-illustration">"✳"</div>
                                                        <h2>"Tout commence par une observation."</h2>
                                                        <p>"Ajoutez des faits, une hypothèse ou une question à explorer. Vous pourrez enrichir votre analyse au fil des publications."</p>
                                                    </div>
                                                }
                                            >
                                                <div class="document-body markdown-body" inner_html=preview_html />
                                            </Show>
                                        }
                                    >
                                        <div class="writing-area">
                                            <label class="writing-label" for="analysis-content">"VOTRE ANALYSE"</label>
                                            <textarea
                                                id="analysis-content"
                                                class="writing-input"
                                                aria-label="Contenu de l’analyse"
                                                placeholder="Quelle est l’observation ? Qu’est-ce que cela change dans votre lecture de la devise ? Notez aussi les éléments qui pourraient contredire votre hypothèse."
                                                prop:value=move || content.get()
                                                on:input=move |event| {
                                                    set_content.set(event_target_value(&event));
                                                    set_save_status.set("MODIFICATIONS NON ENREGISTRÉES".to_string());
                                                }
                                            ></textarea>
                                            <div class="writing-footer">
                                                <span>"Écrivez librement ; les sauts de ligne sont conservés."</span>
                                                <span>{character_count} " caractères"</span>
                                            </div>
                                        </div>
                                    </Show>
                                </article>
                            </section>
                        }.into_any()
                    } else if active_section.get() == "currencies"
                        && selected_currency.get().is_some()
                    {
                        let currency = selected_currency
                            .get()
                            .and_then(|code| currency_info(&code))
                            .unwrap_or(UNASSIGNED_CURRENCY);
                        view! {
                            <section class="currency-page">
                                <div class="currency-back-row">
                                    <button class="back-link" on:click=move |_| {
                                        set_active_section.set("currencies".to_string());
                                        set_selected_currency.set(None);
                                        set_editor_open.set(false);
                                        set_search.set(String::new());
                                    }>
                                        <span aria-hidden="true">"←"</span> "Tous les dossiers"
                                    </button>
                                    <span class="scope-indicator"><span>"●"</span> "ESPACE INDÉPENDANT"</span>
                                </div>

                                <div class="currency-hero">
                                    <img class="currency-hero-flag" src=format!("/flags/{}.png", currency.flag) alt=format!("Drapeau : {}", currency.country)/>
                                    <div class="currency-identity">
                                        <div class="eyebrow">{if currency.external { "FACTEUR EXTERNE" } else { "DOSSIER DEVISE" }}</div>
                                        <h1>{currency.name} <span>{currency.code}</span></h1>
                                        <p>{currency.country} <span>"·"</span> {currency.bank}</p>
                                    </div>
                                    <button class="button button-primary currency-new-note" on:click=new_note>
                                        <span>"＋"</span> "Nouvelle note"
                                    </button>
                                </div>

                                <div class="currency-notice">
                                    <span class="notice-icon">"✳"</span>
                                    <span>"Ce dossier contient uniquement les notes liées à " {currency.code} {". Les analyses restent séparées entre devises."}</span>
                                </div>

                                <div class="currency-market-grid">
                                    <section class="cot-card">
                                        <div class="cot-card-heading">
                                            <div>
                                                <div class="eyebrow">"POSITIONNEMENT · CFTC"</div>
                                                <h2>"Données COT"</h2>
                                            </div>
                                            <span class="cot-indicator-label">"INDICATEUR, PAS SIGNAL AUTONOME"</span>
                                        </div>
                                        {move || {
                                            match cot_manifest.get() {
                                                None => view! {
                                                    <p class="cot-state">"Chargement des données COT…"</p>
                                                }.into_any(),
                                                Some(Err(error)) => view! {
                                                    <p class="cot-state cot-state-error" role="alert">
                                                        "Impossible de charger le graphique COT : " {error}
                                                    </p>
                                                }.into_any(),
                                                Some(Ok(markets)) => {
                                                    match markets.into_iter().find(|market| market.currency == currency.code) {
                                                        Some(market) => view! {
                                                            <div class="cot-chart-meta">
                                                                <span>"Dernière date de référence"</span>
                                                                <strong>{format_date(&market.latest_date)}</strong>
                                                            </div>
                                                            {if market.is_stale {
                                                                view! {
                                                                    <p class="cot-stale-notice">
                                                                        "Graphique masqué : la série Python est ancienne et ne représente pas le positionnement USD actuel."
                                                                    </p>
                                                                }.into_any()
                                                            } else {
                                                                ().into_any()
                                                            }}
                                                            {if market.is_usd_proxy {
                                                                view! {
                                                                    <p class="cot-proxy-note">
                                                                        "USD : proxy pondéré du Dollar Index (DXY)."
                                                                    </p>
                                                                }.into_any()
                                                            } else {
                                                                ().into_any()
                                                            }}
                                                            {if market.is_usd_aggregate {
                                                                view! {
                                                                    <p class="cot-proxy-note">
                                                                        "Proxy COT USD pondéré façon DXY, SEK-free : −(57,6 % EUR + 13,6 % JPY + 11,9 % GBP + 9,1 % CAD + 3,6 % CHF). Ces poids couvrent 95,8 % du DXY; les 4,2 % SEK sont exclus sans renormalisation faute de série COT SEK. Approximation construite à partir du graphique de formation, pas une série USD publiée par la CFTC."
                                                                    </p>
                                                                }.into_any()
                                                            } else {
                                                                ().into_any()
                                                            }}
                                                            {if market.is_stale {
                                                                ().into_any()
                                                            } else {
                                                                view! {
                                                                    <a class="cot-chart-link" href=format!("/cot/{}", market.chart_file) target="_blank" rel="noopener noreferrer">
                                                                        <img
                                                                            class="cot-chart-image"
                                                                            src=format!("/cot/{}", market.chart_file)
                                                                            alt=format!("Graphique COT {} — {}", currency.code, market.market_name)
                                                                            loading="lazy"
                                                                        />
                                                                    </a>
                                                                }.into_any()
                                                            }}
                                                        }.into_any(),
                                                        None => view! {
                                                            <p class="cot-state">
                                                                "Aucune série COT suivie pour " {currency.code} "."
                                                            </p>
                                                        }.into_any(),
                                                    }
                                                }
                                            }
                                        }}
                                    </section>

                                    <section class="currency-rates-card">
                                        <div class="eyebrow">"PRICING · TAUX IMPLICITES"</div>
                                        <h2>"Anticipations de taux"</h2>
                                        <p>
                                            "Consulte les anticipations de marché pour " {currency.code}
                                            " sur Rate Probability."
                                        </p>
                                        <a
                                            class="currency-rate-link"
                                            href="https://rateprobability.com/"
                                            target="_blank"
                                            rel="noopener noreferrer"
                                        >
                                            <span class="resource-mark">"↗"</span>
                                            <span>
                                                <strong>"Ouvrir Rate Probability"</strong>
                                                <small>"Données et probabilités de taux"</small>
                                            </span>
                                            <span class="resource-arrow">"↗"</span>
                                        </a>
                                    </section>
                                </div>

                                    <section class="currency-rates-card">
                                        <div class="eyebrow">"FONDAMENTAUX · SOURCES EXTERNES"</div>
                                        <h2>"Données économiques"</h2>
                                        <p>"Inflation, chômage, PIB, PMI et taux directeurs suivis en ligne sur Trading Economics, sans saisie locale."</p>
                                        <a
                                            class="currency-rate-link"
                                            href=format!("https://tradingeconomics.com/{}/indicators", tradingeconomics_slug(currency.code))
                                            target="_blank"
                                            rel="noopener noreferrer"
                                        >
                                            <span class="resource-mark">"↗"</span>
                                            <span>
                                                <strong>"Indicateurs économiques"</strong>
                                                <small>"Inflation, chômage, PIB, PMI…"</small>
                                            </span>
                                            <span class="resource-arrow">"↗"</span>
                                        </a>
                                        <a
                                            class="currency-rate-link"
                                            href="https://tradingeconomics.com/calendar"
                                            target="_blank"
                                            rel="noopener noreferrer"
                                        >
                                            <span class="resource-mark">"↗"</span>
                                            <span>
                                                <strong>"Calendrier économique"</strong>
                                                <small>"Publications à venir par pays"</small>
                                            </span>
                                            <span class="resource-arrow">"↗"</span>
                                        </a>
                                    </section>


                                <section class="currency-notes">
                                    <div class="currency-notes-heading">
                                        <div>
                                            <div class="eyebrow">"HISTORIQUE DU DOSSIER"</div>
                                            <h2>"Notes de recherche"</h2>
                                        </div>
                                        <label class="search-box">
                                            <span class="search-icon" aria-hidden="true">"⌕"</span>
                                            <input
                                                type="search"
                                                aria-label=format!("Rechercher dans les notes {}", currency.code)
                                                placeholder="Rechercher dans ce dossier…"
                                                prop:value=move || search.get()
                                                on:input=move |event| set_search.set(event_target_value(&event))
                                            />
                                        </label>
                                    </div>
                                    <Show
                                        when=move || !filtered_entries().is_empty()
                                        fallback=move || view! {
                                            <div class="empty-journal currency-empty">
                                                <div class="empty-journal-icon">{move || if entries.get().is_empty() { "✳" } else { "⌕" }}</div>
                                                <h3>{move || if entries.get().is_empty() { format!("Aucune note dans le dossier {} pour le moment.", currency.code) } else { "Aucune note ne correspond à cette recherche.".to_string() }}</h3>
                                                <p>{move || if entries.get().is_empty() { "Consignez ici les données, décisions de banque centrale et hypothèses propres à cette devise.".to_string() } else { "Essayez un autre mot-clé ou effacez votre recherche.".to_string() }}</p>
                                                <Show when=move || entries.get().is_empty()>
                                                    <button class="text-action" on:click=new_note>"Créer la première note" <span>"→"</span></button>
                                                </Show>
                                            </div>
                                        }
                                    >
                                        <div class="notes-grid">
                                            <For
                                                each=move || filtered_entries()
                                                key=|entry| entry.id
                                                children=move |entry| {
                                                    let note_title = entry.title.clone();
                                                    let note_content = entry.content.clone();
                                                    let updated = entry.updated_at.clone();
                                                    let snippet = preview_text(&entry.content);
                                                    view! {
                                                        <button
                                                            class="research-card"
                                                            on:click=move |_| {
                                                                set_active_id.set(Some(entry.id));
                                                                set_title.set(note_title.clone());
                                                                set_content.set(note_content.clone());
                                                                set_editor_open.set(true);
                                                                set_editing.set(false);
                                                                set_preview_mode.set(false);
                                                                set_save_status.set(format!("OUVERT · {}", format_date(&updated)));
                                                            }
                                                        >
                                                            <div class="research-card-top">
                                                                <span class="research-card-type"><span>"✳"</span> {currency.code} " · ANALYSE"</span>
                                                                <span class="card-arrow" aria-hidden="true">"↗"</span>
                                                            </div>
                                                            <h3>{entry.title.clone()}</h3>
                                                            <p class="research-card-excerpt">{if snippet.is_empty() { "Aucun contenu pour le moment.".to_string() } else { snippet }}</p>
                                                            <div class="research-card-footer">
                                                                <span class="card-date"><span>"◷"</span> {format_date(&entry.updated_at)}</span>
                                                                <span class="card-read">"Ouvrir la note"</span>
                                                            </div>
                                                        </button>
                                                    }
                                                }
                                            />
                                        </div>
                                    </Show>
                                </section>
                            </section>
                        }.into_any()
                    } else if active_section.get() == "currencies" {
                        view! {
                            <section class="currency-index-page">
                                <div class="currency-index-intro">
                                    <div class="eyebrow"><span class="eyebrow-dot"></span> "RECHERCHE FONDAMENTALE · VOS DOSSIERS"</div>
                                    <h1>"Un dossier de recherche par devise."</h1>
                                    <p>"Chaque économie dispose de son propre dossier. Les analyses restent cloisonnées par devise ; les comparaisons directes entre devises sont prévues dans une itération ultérieure."</p>
                                </div>

                                <div class="currency-group-heading">
                                    <div>
                                        <div class="eyebrow">"LES 8 DEVISES MAJEURES"</div>
                                        <h2>"Sélectionnez un dossier de recherche"</h2>
                                    </div>
                                    <span class="folder-count">"08 DOSSIERS"</span>
                                </div>

                                <div class="currency-grid">
                                    <For
                                        each=move || {
                                            CURRENCIES.iter().filter(|currency| !currency.external).copied().collect::<Vec<_>>()
                                        }
                                        key=|currency| currency.code
                                        children=move |currency| {
                                            view! {
                                                <button class="currency-card" on:click=move |_| open_currency(currency.code)>
                                                    <div class="currency-card-top">
                                                        <img class="currency-card-flag" src=format!("/flags/{}.png", currency.flag) alt=format!("Drapeau : {}", currency.country)/>
                                                        <span class="currency-card-arrow" aria-hidden="true">"↗"</span>
                                                    </div>
                                                    <div class="currency-card-code">{currency.code}</div>
                                                    <h3>{currency.name}</h3>
                                                    <p>{currency.country}</p>
                                                    <div class="currency-card-bottom">
                                                        <span>{currency.bank}</span>
                                                        <span>"OUVRIR"</span>
                                                    </div>
                                                </button>
                                            }
                                        }
                                    />
                                </div>

                                <div class="external-heading">
                                    <div>
                                        <div class="eyebrow">"FACTEUR EXTERNE"</div>
                                        <h2>"À surveiller"</h2>
                                    </div>
                                    <p>"La Chine, facteur externe notable pour l'AUD et le NZD."</p>
                                </div>
                                <For
                                    each=move || {
                                        CURRENCIES.iter().filter(|currency| currency.external).copied().collect::<Vec<_>>()
                                    }
                                    key=|currency| currency.code
                                    children=move |currency| {
                                        view! {
                                            <button class="currency-card external-currency-card" on:click=move |_| open_currency(currency.code)>
                                                <div class="currency-card-top">
                                                    <img class="currency-card-flag" src=format!("/flags/{}.png", currency.flag) alt=format!("Drapeau : {}", currency.country)/>
                                                    <span class="external-badge">"CONTEXTE EXTERNE"</span>
                                                    <span class="currency-card-arrow" aria-hidden="true">"↗"</span>
                                                </div>
                                                <div class="currency-card-code">{currency.code}</div>
                                                <h3>{currency.name}</h3>
                                                <p>{currency.country}</p>
                                                <div class="currency-card-bottom">
                                                    <span>{currency.bank}</span>
                                                    <span>"OUVRIR"</span>
                                                </div>
                                            </button>
                                        }
                                    }
                                />

                                <button class="unassigned-link" on:click=open_unassigned>
                                    <span class="unassigned-mark">"↗"</span>
                                    <span><strong>"Notes à classer"</strong><small>"Toutes les notes, toutes devises : filtrez puis recherchez."</small></span>
                                    <span class="currency-card-arrow">"→"</span>
                                </button>
                            </section>
                        }.into_any()
                    } else if active_section.get() == "journal" {
                        view! {
                            <section class="dashboard-page">
                                <div class="welcome-banner">
                                    <div class="welcome-copy">
                                        <div class="eyebrow"><span class="eyebrow-dot"></span> "HUB DE RECHERCHE · TOUTES DEVISES"</div>
                                        <h1>"Toutes vos notes, tous les dossiers."<br/><span>"Filtrez et recherchez dans l'ensemble des devises."</span></h1>
                                        <p>"Les analyses de chaque dossier de devise se retrouvent ici, notes à classer comprises. Sélectionnez une ou plusieurs devises, puis effectuez votre recherche."</p>
                                    </div>
                                    <div class="welcome-art" aria-hidden="true">
                                        <div class="art-orbit orbit-one"></div>
                                        <div class="art-orbit orbit-two"></div>
                                        <div class="art-core"><span>"MJ"</span></div>
                                        <div class="art-spark spark-one"></div>
                                        <div class="art-spark spark-two"></div>
                                        <div class="art-caption">"FONDAMENTAUX · POSITIONNEMENT · PRIX"</div>
                                    </div>
                                </div>

                                <div class="stats-grid">
                                    <article class="stat-card">
                                        <div class="stat-label"><span class="stat-icon">"▤"</span> "TOUTES DEVISES"</div>
                                        <div class="stat-value">{move || entries.get().len()}</div>
                                        <div class="stat-detail">"toutes devises et notes à classer confondues"</div>
                                    </article>
                                    <article class="stat-card">
                                        <div class="stat-label"><span class="stat-icon stat-icon-sand">"◷"</span> "DERNIÈRE ACTIVITÉ"</div>
                                        <div class="stat-value stat-date">{move || {
                                            entries.get().first()
                                                .map(|entry| format_date(&entry.updated_at))
                                                .unwrap_or_else(|| "—".to_string())
                                        }}</div>
                                        <div class="stat-detail">{move || if entries.get().is_empty() { "Aucune note enregistrée pour le moment" } else { "Dernière mise à jour enregistrée" }}</div>
                                    </article>
                                    <article class="stat-card stat-card-local">
                                        <div class="stat-label"><span class="stat-icon stat-icon-green">"⌂"</span> "VOTRE ESPACE"</div>
                                        <div class="stat-value stat-local">{move || if api_status.get() { "Local" } else { "Hors ligne" }}</div>
                                        <div class="stat-detail">{move || if api_status.get() { "Données conservées sur cet appareil" } else { "Connexion à la base locale indisponible" }}</div>
                                    </article>
                                </div>

                                <div class="dashboard-columns">
                                    <section class="research-section">
                                        <div class="section-heading">
                                            <div>
                                                <div class="eyebrow">"TOUTES DEVISES · RECHERCHE"</div>
                                                <h2>"Recherchez dans toutes vos notes"</h2>
                                                <p>"Aucune sélection de devise = tout voir. Le filtre et la recherche se combinent."</p>
                                            </div>
                                            <label class="search-box">
                                                <span class="search-icon" aria-hidden="true">"⌕"</span>
                                                <input
                                                    type="search"
                                                    aria-label="Rechercher dans les notes"
                                                    placeholder="Rechercher une note…"
                                                    prop:value=move || search.get()
                                                    on:input=move |event| set_search.set(event_target_value(&event))
                                                />
                                            </label>
                                        </div>

                                        <div class="journal-filter">
                                            <div class="journal-filter-chips">
                                                <For
                                                    each=move || {
                                                        CURRENCIES.to_vec()
                                                    }
                                                    key=|currency| currency.code
                                                    children=move |currency| {
                                                        let code = currency.code;
                                                        view! {
                                                            <button
                                                                class="journal-chip"
                                                                class:active=move || currency_filter.get().contains(code)
                                                                on:click=move |_| toggle_currency_filter(code)
                                                            >
                                                                <img src=format!("/flags/{}.png", currency.flag) alt=currency.code/>
                                                                <span>{currency.code}</span>
                                                            </button>
                                                        }
                                                    }
                                                />
                                                <button
                                                    class="journal-chip journal-chip-unassigned"
                                                    class:active=move || currency_filter.get().contains("UNASSIGNED")
                                                    on:click=move |_| toggle_currency_filter("UNASSIGNED")
                                                >
                                                    <span>"À classer"</span>
                                                </button>
                                                <button class="journal-chip-reset" on:click=clear_currency_filter>"Tout"</button>
                                            </div>
                                            <span class="journal-filter-hint">{move || {
                                                let selected = currency_filter.get();
                                                if selected.is_empty() {
                                                    "AUCUNE SÉLECTION · TOUTES LES NOTES".to_string()
                                                } else {
                                                    format!("{} DEVISE(S) SÉLECTIONNÉE(S)", selected.len())
                                                }
                                            }}</span>
                                        </div>

                                        <Show
                                            when=move || !filtered_entries().is_empty()
                                            fallback=move || view! {
                                                <div class="empty-journal">
                                                    <div class="empty-journal-icon">{move || if entries.get().is_empty() { "✳" } else { "⌕" }}</div>
                                                    <h3>{move || if entries.get().is_empty() { "Aucune note enregistrée pour le moment." } else { "Aucune note pour cette sélection." }}</h3>
                                                    <p>{move || if entries.get().is_empty() { "Les notes créées à partir de maintenant seront enregistrées dans le dossier de la devise concernée.".to_string() } else { "Essayez un autre mot-clé, ou ajustez le filtre de devises.".to_string() }}</p>
                                                </div>
                                            }
                                        >
                                            <div class="notes-grid">
                                                <For
                                                    each=move || filtered_entries()
                                                    key=|entry| entry.id
                                                    children=move |entry| {
                                                        let note_title = entry.title.clone();
                                                        let note_content = entry.content.clone();
                                                        let note_currency = entry.currency_code.clone();
                                                        let note_badge = if note_currency == "UNASSIGNED" {
                                                            "À CLASSER".to_string()
                                                        } else {
                                                            format!("{} · ANALYSE", note_currency)
                                                        };
                                                        let updated = entry.updated_at.clone();
                                                        let snippet = preview_text(&entry.content);
                                                        view! {
                                                            <button
                                                                class="research-card"
                                                                on:click=move |_| {
                                                                    set_active_id.set(Some(entry.id));
                                                                    set_selected_currency.set(
                                                                        if note_currency == "UNASSIGNED" {
                                                                            None
                                                                        } else {
                                                                            Some(note_currency.clone())
                                                                        },
                                                                    );
                                                                    set_title.set(note_title.clone());
                                                                    set_content.set(note_content.clone());
                                                                    set_editor_open.set(true);
                                                                    set_editing.set(false);
                                                                    set_preview_mode.set(false);
                                                                    set_save_status.set(format!("OUVERT · {}", format_date(&updated)));
                                                                }
                                                            >
                                                                <div class="research-card-top">
                                                                    <span class="research-card-type"><span>"✳"</span> {note_badge}</span>
                                                                    <span class="card-arrow" aria-hidden="true">"↗"</span>
                                                                </div>
                                                                <h3>{entry.title.clone()}</h3>
                                                                <p class="research-card-excerpt">{if snippet.is_empty() { "Aucun contenu pour le moment.".to_string() } else { snippet }}</p>
                                                                <div class="research-card-footer">
                                                                    <span class="card-date"><span>"◷"</span> {format_date(&entry.updated_at)}</span>
                                                                    <span class="card-read">"Ouvrir la note"</span>
                                                                </div>
                                                            </button>
                                                        }
                                                    }
                                                />
                                            </div>
                                        </Show>
                                    </section>

                                    <aside class="guidance-column">
                                        <section class="method-card">
                                            <div class="method-heading">
                                                <span class="method-icon">"✳"</span>
                                                <span class="eyebrow">"VOTRE CADRE D’ANALYSE"</span>
                                            </div>
                                            <h2>"Ce qui fonde une conviction."</h2>
                                            <p>"Pas de signal magique. Une lecture solide relie plusieurs angles et garde la place au doute."</p>
                                            <div class="method-steps">
                                                <div class="method-step">
                                                    <span class="step-number">"01"</span>
                                                    <div><strong>"Les fondamentaux"</strong><span>"Taux, inflation, emploi, croissance"</span></div>
                                                </div>
                                                <div class="method-step">
                                                    <span class="step-number">"02"</span>
                                                    <div><strong>"Le contexte de marché"</strong><span>"Positionnement, corrélations, sentiment"</span></div>
                                                </div>
                                                <div class="method-step">
                                                    <span class="step-number">"03"</span>
                                                    <div><strong>"Votre raisonnement"</strong><span>"Faits, hypothèse, contre-arguments"</span></div>
                                                </div>
                                            </div>
                                        </section>

                                    </aside>
                                </div>
                            </section>
                        }.into_any()
                    } else if active_section.get() == "weekly" {
                        view! {
                            <WeeklySection cot_manifest=cot_manifest/>
                        }.into_any()
                    } else if active_section.get() == "pairs" {
                        view! {
                            <PairsSection/>
                        }.into_any()
                    } else if active_section.get() == "reviews" {
                        view! {
                            <ReviewsSection/>
                        }.into_any()
                    } else {
                        view! {
                            <section class="placeholder-page">
                                <div class="eyebrow">"MACRO JOURNAL · ESPACE DE TRAVAIL"</div>
                                <div class="placeholder-mark">"MJ"</div>
                                <h1>{section_title}</h1>
                                <p>"Cet espace fait partie de la structure prévue. Il sera développé lorsque le socle du journal sera stabilisé."</p>
                                <button class="button button-primary" on:click=move |_| set_active_section.set("journal".to_string())>
                                    "Retour au journal de recherche"
                                </button>
                            </section>
                        }.into_any()
                    }
                }}

                <footer class="app-footer">
                    <span><span class="footer-dot" class:online=move || api_status.get()></span> {move || if api_status.get() { "Base locale connectée" } else { "En attente de la base locale" }}</span>
                    <span>"VOS NOTES RESTENT SUR CET APPAREIL"</span>
                </footer>
            </main>
        </div>
    }
}

fn format_date(timestamp: &str) -> String {
    let date = timestamp.split('T').next().unwrap_or(timestamp);
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() == 3 {
        format!("{}/{}/{}", parts[2], parts[1], parts[0])
    } else {
        date.to_string()
    }
}

fn push_inline_preview(target: &mut String, addition: &str) {
    let addition = addition.trim();
    if addition.is_empty() {
        return;
    }
    if !target.is_empty() && !target.ends_with(' ') {
        target.push(' ');
    }
    target.push_str(addition);
}

/// Aperçu d'une note pour les cartes du hub : le markdown est aplati,
/// mais chaque bloc (titre, paragraphe, élément de liste, cellule de
/// tableau) forme une section séparée — sinon le libellé de la section
/// colle à la réponse écrite juste en dessous.
fn preview_text(source: &str) -> String {
    /// Séparateur d'aperçu entre deux blocs.
    const PREVIEW_SEPARATOR: &str = " · ";

    fn flush_segment(current: &mut String, segments: &mut Vec<String>) {
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            segments.push(trimmed.to_string());
        }
        current.clear();
    }

    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Text(text) => push_inline_preview(&mut current, &text),
            Event::Code(code) => push_inline_preview(&mut current, &code),
            // Case manuelle des fiches (checklist) : rendue littéralement,
            // attachée au libellé qui suit.
            Event::TaskListMarker(complete) => {
                current.push_str(if complete { "[x] " } else { "[ ] " });
            }
            Event::SoftBreak | Event::HardBreak => {
                if !current.is_empty() && !current.ends_with(' ') {
                    current.push(' ');
                }
            }
            Event::Start(tag) => match tag {
                Tag::Emphasis
                | Tag::Strong
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. } => {}
                _ => flush_segment(&mut current, &mut segments),
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image => {}
                _ => flush_segment(&mut current, &mut segments),
            },
            _ => {}
        }
    }
    flush_segment(&mut current, &mut segments);

    let joined = segments.join(PREVIEW_SEPARATOR);
    let preview = joined.chars().take(150).collect::<String>();
    if joined.chars().count() > 150 {
        format!("{preview}…")
    } else {
        preview
    }
}

async fn check_api() -> Result<(), String> {
    let response = Request::get(&format!("{API_BASE}/api/health"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.ok() {
        Ok(())
    } else {
        Err(format!("API returned HTTP {}", response.status()))
    }
}

async fn fetch_entries(currency_code: &str) -> Result<Vec<Entry>, String> {
    let response = Request::get(&format!(
        "{API_BASE}/api/currencies/{currency_code}/entries"
    ))
    .send()
    .await
    .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }

    response
        .json::<Vec<Entry>>()
        .await
        .map_err(|error| error.to_string())
}

async fn fetch_all_entries() -> Result<Vec<Entry>, String> {
    let response = Request::get(&format!("{API_BASE}/api/entries"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }
    response
        .json::<Vec<Entry>>()
        .await
        .map_err(|error| error.to_string())
}

/// Notes visibles dans le hub « notes à classer » : aucune devise
/// sélectionnée = toutes ; sinon uniquement les devises choisies
/// (UNASSIGNED inclus). La requête porte sur le titre et le contenu.
fn entries_matching_filter(all: &[Entry], selected: &HashSet<String>, query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    all.iter()
        .filter(|entry| {
            (selected.is_empty() || selected.contains(&entry.currency_code))
                && (query.is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

async fn fetch_cot_manifest() -> Result<Vec<CotChartAsset>, String> {
    let response = Request::get("/cot/manifest.json")
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }

    response
        .json::<CotManifest>()
        .await
        .map(|manifest| manifest.markets)
        .map_err(|error| error.to_string())
}

async fn fetch_economic_observations(
    currency_code: &str,
) -> Result<Vec<EconomicObservation>, String> {
    let response = Request::get(&format!(
        "{API_BASE}/api/economic/{currency_code}/observations"
    ))
    .send()
    .await
    .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }

    response
        .json::<Vec<EconomicObservation>>()
        .await
        .map_err(|error| error.to_string())
}

async fn create_entry(currency_code: &str, draft: EntryDraft) -> Result<Entry, String> {
    let response = Request::post(&format!(
        "{API_BASE}/api/currencies/{currency_code}/entries"
    ))
    .json(&draft)
    .map_err(|error| error.to_string())?
    .send()
    .await
    .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }

    response
        .json::<Entry>()
        .await
        .map_err(|error| error.to_string())
}

/// Supprime définitivement une entrée (le serveur répond 204).
async fn delete_entry_request(id: i64) -> Result<(), String> {
    let response = Request::delete(&format!("{API_BASE}/api/entries/{id}"))
        .send()
        .await
        .map_err(|error| error.to_string())?;

    match response.ok() {
        true => Ok(()),
        false => Err(format!("API returned HTTP {}", response.status())),
    }
}

async fn update_entry(id: i64, currency_code: &str, draft: EntryDraft) -> Result<Entry, String> {
    let response = Request::put(&format!(
        "{API_BASE}/api/currencies/{currency_code}/entries/{id}"
    ))
    .json(&draft)
    .map_err(|error| error.to_string())?
    .send()
    .await
    .map_err(|error| error.to_string())?;

    if !response.ok() {
        return Err(format!("API returned HTTP {}", response.status()));
    }

    response
        .json::<Entry>()
        .await
        .map_err(|error| error.to_string())
}

fn render_markdown(source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(source, options);
    let mut unsafe_html = String::new();
    html::push_html(&mut unsafe_html, parser);

    ammonia::clean(&unsafe_html)
}

/// Garde-fous de l'onglet préparation hebdomadaire (veille).
#[cfg(test)]
mod veille_tests {
    use super::*;

    #[test]
    fn veille_links_json_parses_into_groups() {
        let text = include_str!("../public/veille/links.json");
        let resources = serde_json::from_str::<VeilleResources>(text)
            .expect("links.json doit être { \"groups\": [...] }");
        let groups = resources.groups;
        assert!(groups.len() >= 4, "au moins les 4 blocs du rituel");
        for group in &groups {
            assert!(!group.links.is_empty(), "groupe {} sans lien", group.id);
            for link in &group.links {
                assert!(
                    link.url.starts_with("https://"),
                    "lien non https : {}",
                    link.url
                );
                assert!(!link.title.is_empty());
            }
        }
        let ids: Vec<&str> = groups.iter().map(|group| group.id.as_str()).collect();
        assert!(ids.contains(&"morning"));
        assert!(ids.contains(&"cot-friday"));
        assert!(ids.contains(&"weekahead"));
        assert!(ids.contains(&"banks"));
    }

    fn observation(series_id: &str, date: &str, value: f64) -> EconomicObservation {
        EconomicObservation {
            series_id: series_id.to_string(),
            date: date.to_string(),
            value,
            value_secondary: None,
        }
    }

    fn practical_group() -> Vec<ResourceGroup> {
        vec![ResourceGroup {
            id: "morning".to_string(),
            title: "Brief du matin".to_string(),
            timing: "tous les jours".to_string(),
            description: "…".to_string(),
            links: vec![ResourceLink {
                title: "Trading Economics — Calendrier".to_string(),
                url: "https://tradingeconomics.com/calendar".to_string(),
                note: String::new(),
            }],
        }]
    }

    fn sample_cot() -> Vec<CotChartAsset> {
        vec![CotChartAsset {
            currency: "EUR".to_string(),
            market_name: "Euro FX".to_string(),
            chart_file: "eur_net_noncomm_pct_oi.png".to_string(),
            latest_date: "2026-10-27".to_string(),
            is_usd_proxy: false,
            is_usd_aggregate: false,
            is_stale: true,
        }]
    }

    #[test]
    fn recent_series_keeps_the_two_latest_observations_by_date() {
        let observations = vec![
            observation("policy_rate", "2026-07-31", 2.50),
            observation("policy_rate", "2026-09-30", 2.25),
            observation("policy_rate", "2026-05-31", 2.75),
            observation("unemployment_rate", "2026-09-01", 9.2),
        ];
        let recent = recent_series(&observations, &["policy_rate"]);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].series_id, "policy_rate");
        assert_eq!(recent[0].last.date, "2026-09-30");
        assert_eq!(recent[0].last.value, 2.25);
        let prev = recent[0].prev.as_ref().expect("2e observation attendue");
        assert_eq!(prev.date, "2026-07-31");
        assert_eq!(prev.value, 2.50);
    }

    #[test]
    fn weekly_pack_contains_cot_series_and_sources() {
        let snapshots = vec![CurrencySnapshot {
            code: "EUR",
            series: vec![RecentSeries {
                series_id: "policy_rate".to_string(),
                last: observation("policy_rate", "2026-09-30", 2.25),
                prev: Some(observation("policy_rate", "2026-07-31", 2.50)),
            }],
            journal_note: Some("ECB face aux tarifs (2026-11-05)".to_string()),
        }];
        let pack = compose_weekly_pack(Some(&sample_cot()), &snapshots, &practical_group());
        assert!(pack.contains("## Positionnement COT"));
        assert!(pack.contains("- EUR — dernière observation 2026-10-27 · observation périmée"));
        assert!(pack.contains("policy_rate : 2.25 (2026-09-30) | avant 2.5 (2026-07-31)"));
        assert!(pack.contains("- Note journal : ECB"));
        assert!(pack.contains("tradingeconomics.com"));
        assert!(pack.contains("biais court / moyen / long"));
    }

    #[test]
    fn weekly_pack_degrades_without_api_and_manifest() {
        let pack = compose_weekly_pack(None, &[], &[]);
        assert!(pack.contains("Indisponible"));
    }

    #[test]
    fn morning_pack_lists_morning_sources_and_cot_status() {
        let pack = compose_morning_pack(Some(&sample_cot()), &practical_group());
        assert!(pack.contains("risk-on"));
        assert!(pack.contains("https://tradingeconomics.com/calendar"));
        assert!(pack.contains("- EUR — dernière observation 2026-10-27"));
    }
}

/// Garde-fous du hub de recherche de l'onglet notes à classer.
#[cfg(test)]
mod journal_filter_tests {
    use super::*;

    fn entry(id: i64, currency_code: &str, title: &str, content: &str) -> Entry {
        Entry {
            id,
            currency_code: currency_code.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            created_at: "2026-11-01T10:00:00Z".to_string(),
            updated_at: "2026-11-01T10:00:00Z".to_string(),
        }
    }

    fn selected(codes: &[&str]) -> HashSet<String> {
        codes.iter().map(|code| (*code).to_string()).collect()
    }

    #[test]
    fn empty_selection_returns_every_note() {
        let all = vec![
            entry(1, "USD", "Élections US", "Analyse du vote."),
            entry(2, "EUR", "HICP", "Inflation sous-jacente."),
            entry(3, "UNASSIGNED", "À trier", "Brouillon."),
        ];
        let visible = entries_matching_filter(&all, &selected(&[]), "");
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn multi_currency_selection_filters_both() {
        let all = vec![
            entry(1, "USD", "Élections US", "Analyse du vote."),
            entry(2, "EUR", "HICP", "Inflation sous-jacente."),
            entry(3, "GBP", "MPC", "Vote de la Bank."),
            entry(4, "UNASSIGNED", "À trier", "Brouillon."),
        ];
        let visible = entries_matching_filter(&all, &selected(&["USD", "EUR"]), "");
        let ids: Vec<i64> = visible.iter().map(|entry| entry.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn unassigned_chip_selects_only_unassigned_notes() {
        let all = vec![
            entry(1, "USD", "Élections US", "Analyse du vote."),
            entry(2, "UNASSIGNED", "À trier", "Brouillon."),
        ];
        let visible = entries_matching_filter(&all, &selected(&["UNASSIGNED"]), "");
        let ids: Vec<i64> = visible.iter().map(|entry| entry.id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn search_applies_within_selected_currencies() {
        let all = vec![
            entry(1, "USD", "Élections US", "Le taux franchit la borne haute."),
            entry(2, "EUR", "HICP", "Un taux d'inflation qui Surprend."),
        ];
        let visible = entries_matching_filter(&all, &selected(&["USD"]), "taux");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, 1);
    }

    #[test]
    fn search_matches_title_or_content_case_insensitively() {
        let all = vec![
            entry(1, "USD", "Élections US", "Analyse du vote."),
            entry(2, "EUR", "HICP", "Inflation sous-jacente."),
        ];
        assert_eq!(
            entries_matching_filter(&all, &selected(&[]), "élections")[0].id,
            1
        );
        assert_eq!(
            entries_matching_filter(&all, &selected(&[]), "INFLATION")[0].id,
            2
        );
        assert!(entries_matching_filter(&all, &selected(&[]), "inexistant").is_empty());
    }
}

/// Garde-fous de la lecture fondamentale (analyse des paires + fiche).
#[cfg(test)]
mod pairs_tests {
    use super::*;

    fn observation(series_id: &str, date: &str, value: f64) -> EconomicObservation {
        EconomicObservation {
            series_id: series_id.to_string(),
            date: date.to_string(),
            value,
            value_secondary: None,
        }
    }

    #[test]
    fn last_two_observations_orders_by_date_desc() {
        let observations = vec![
            observation("policy_rate", "2026-07-31", 2.50),
            observation("policy_rate", "2026-09-30", 2.25),
            observation("policy_rate", "2026-05-31", 2.75),
        ];
        let (last, previous) = last_two_observations(&observations, "policy_rate");
        assert_eq!(last.map(|observation| observation.value), Some(2.25));
        assert_eq!(previous.map(|observation| observation.value), Some(2.50));
    }

    #[test]
    fn build_fiche_content_marks_checked_items_only() {
        let checked: HashSet<String> = ["bc", "cot"].iter().map(|id| id.to_string()).collect();
        let content = build_fiche_content(
            "USD",
            &checked,
            "Le dollar reste devant, à surveiller à l'approche du CPI.",
        );
        assert!(content.contains("# Fiche analyse USD"));
        assert!(content.contains("## Fondamentaux purs"));
        assert!(content.contains("- [x] Banque centrale"));
        assert!(content.contains("- [ ] Inflation"));
        assert!(content.contains("- [x] COT lu"));
        assert!(content.contains("## Ma conclusion"));
        assert!(content.contains("Le dollar reste devant"));
    }

    #[test]
    fn build_fiche_content_handles_blank_conclusion() {
        let checked: HashSet<String> = HashSet::new();
        let content = build_fiche_content("EUR", &checked, "   ");
        assert!(content.contains("Pas d'avis cette semaine."));
        assert!(content.contains("- [ ] Banque centrale"));
        assert!(!content.contains("- [x]"));
    }

    #[test]
    fn pct_with_delta_formatting() {
        assert_eq!(pct_with_delta(None, Some(1.0)), "—");
        assert_eq!(pct_with_delta(Some(2.25), None), "2.25 %");
        assert_eq!(pct_with_delta(Some(2.1), Some(2.5)), "2.10 % (-0.40)");
    }

    #[test]
    fn sort_healths_orders_by_real_rate_then_code() {
        let healths = vec![
            currency_health("JPY", &[]),
            currency_health(
                "EUR",
                &[
                    observation("policy_rate", "2026-09-30", 2.0),
                    observation("hicp_headline_yoy", "2026-09-30", 3.0),
                ],
            ),
            currency_health(
                "USD",
                &[
                    observation("fed_target_range", "2026-09-30", 5.25),
                    observation("pce_inflation", "2026-09-30", 2.6),
                ],
            ),
        ];
        let sorted = sort_healths(healths);
        let codes: Vec<&str> = sorted.iter().map(|health| health.code).collect();
        assert_eq!(codes, vec!["USD", "EUR", "JPY"]);
    }
}

/// Garde-fous du bilan hebdomadaire (onglet Bilans).
#[cfg(test)]
mod bilans_tests {
    use super::*;

    #[test]
    fn bilan_title_targets_currency_or_unassigned() {
        assert_eq!(bilan_title("USD"), "Bilan semaine USD");
        assert_eq!(bilan_title("UNASSIGNED"), "Bilan semaine (à classer)");
    }

    #[test]
    fn build_bilan_content_keeps_every_section() {
        let answers = vec![
            "Le dollar devait rester ferme.".to_string(),
            "Le CPI a surpris au-dessus.".to_string(),
            String::new(),
            "Attendre le pricing avant la paire.".to_string(),
        ];
        let content = build_bilan_content("USD", &answers);
        assert!(content.contains("# Bilan semaine USD"));
        assert!(content.contains("## Ce que vos thèses attendaient"));
        assert!(content.contains("Le CPI a surpris au-dessus."));
        assert!(content.contains("## Ce que je corrige dans ma méthode"));
        assert!(content.contains("« Rien d'écrit. »"));
        assert!(content.contains("Attendre le pricing avant la paire."));
    }

    #[test]
    fn build_bilan_content_flags_blank_sections_only() {
        let answers = vec![String::new(); 4];
        let content = build_bilan_content("UNASSIGNED", &answers);
        assert!(content.contains("# Bilan semaine (à classer)"));
        assert_eq!(content.matches("« Rien d'écrit. »").count(), 4);
        assert!(!content.contains("Attendre"));
    }

    #[test]
    fn sort_verdicts_orders_by_currency_then_series() {
        let verdict = |currency: &str, series: &str| DataVerdict {
            currency: currency.to_string(),
            series_id: series.to_string(),
            block1: "OK".to_string(),
            block2: "OK".to_string(),
            verdict: "OK".to_string(),
            evidence: String::new(),
        };
        let sorted = sort_verdicts(vec![
            verdict("USD", "policy_rate"),
            verdict("EUR", "hicp_core_yoy"),
            verdict("EUR", "hicp_headline_yoy"),
        ]);
        let keys: Vec<(String, String)> = sorted
            .iter()
            .map(|v| (v.currency.clone(), v.series_id.clone()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("EUR".to_string(), "hicp_core_yoy".to_string()),
                ("EUR".to_string(), "hicp_headline_yoy".to_string()),
                ("USD".to_string(), "policy_rate".to_string()),
            ]
        );
    }
}

/// Régression : ce que vous avez vu collé dans les bilans du hub.
#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn preview_text_separates_bilan_sections() {
        let source = "# Bilan semaine USD\n\n## Ce que j'attendais\nUSD fort\n\n## La surprise\nDémocrates US\n\n## La leçon\nÉviter la suranalyse\n";
        assert_eq!(
            preview_text(source),
            "Bilan semaine USD · Ce que j'attendais · USD fort · La surprise · Démocrates US · La leçon · Éviter la suranalyse"
        );
    }

    #[test]
    fn preview_text_keeps_fiche_list_items_separate() {
        let source =
            "# Fiche analyse USD\n\n## Fondamentaux purs\n\n- [x] Banque centrale : décision traitée\n- [ ] Inflation\n\n## Ma conclusion\n\nÀ surveiller après le CPI.\n";
        assert_eq!(
            preview_text(source),
            "Fiche analyse USD · Fondamentaux purs · [x] Banque centrale : décision traitée · [ ] Inflation · Ma conclusion · À surveiller après le CPI."
        );
    }

    #[test]
    fn preview_text_cuts_long_notes_with_ellipsis() {
        let mut source = String::new();
        for _ in 0..40 {
            source.push_str("abcde ");
        }
        let preview = preview_text(&source);
        assert!(preview.ends_with('…'), "{}", preview);
        assert!(preview.starts_with("abcde"));
    }
}
