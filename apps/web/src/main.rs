use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;
use macro_journal_shared::{Entry, EntryDraft};
use pulldown_cmark::{html, Event, Options, Parser};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Copy)]
struct EconomicIndicator {
    id: &'static str,
    title: &'static str,
    unit: &'static str,
    decimals: usize,
    source_name: &'static str,
    source_url: &'static str,
    note: &'static str,
    primary_label: &'static str,
    secondary_label: Option<&'static str>,
    quarterly: bool,
}

const ECONOMIC_INDICATORS: [EconomicIndicator; 9] = [
    EconomicIndicator {
        id: "cpi_inflation",
        title: "Inflation CPI",
        unit: "% sur 12 mois",
        decimals: 1,
        source_name: "Bureau of Labor Statistics",
        source_url: "https://www.bls.gov/cpi/",
        note: "Inflation CPI globale et sous-jacente sur 12 mois, calculée depuis les indices CPI non désaisonnalisés publiés par le BLS. La mesure sous-jacente exclut l’alimentation et l’énergie.",
        primary_label: "CPI global",
        secondary_label: Some("CPI sous-jacent"),
        quarterly: false,
    },
    EconomicIndicator {
        id: "fed_target_range",
        title: "Taux directeur Fed",
        unit: "%",
        decimals: 2,
        source_name: "Federal Reserve Board",
        source_url: "https://www.federalreserve.gov/monetarypolicy/openmarket.htm",
        note: "Fourchette cible du FOMC, relevée mensuellement à partir de la dernière valeur connue.",
        primary_label: "Borne basse",
        secondary_label: Some("Borne haute"),
        quarterly: false,
    },
    EconomicIndicator {
        id: "pce_inflation",
        title: "Inflation PCE",
        unit: "% sur 12 mois",
        decimals: 1,
        source_name: "Bureau of Economic Analysis",
        source_url: "https://www.bea.gov/data/income-saving/personal-income",
        note: "Inflation PCE globale et sous-jacente, calculée sur les indices publiés. Affichage arrondi à un chiffre après la virgule.",
        primary_label: "PCE global",
        secondary_label: Some("PCE sous-jacent"),
        quarterly: false,
    },
    EconomicIndicator {
        id: "ppi_inflation",
        title: "Inflation PPI",
        unit: "% sur 12 mois",
        decimals: 1,
        source_name: "Bureau of Labor Statistics",
        source_url: "https://www.bls.gov/ppi/",
        note: "Inflation PPI globale et sous-jacente sur 12 mois, calculée depuis les indices de demande finale publiés par le BLS. La mesure sous-jacente exclut l’alimentation, l’énergie et les services commerciaux.",
        primary_label: "PPI global",
        secondary_label: Some("PPI sous-jacent"),
        quarterly: false,
    },
    EconomicIndicator {
        id: "unemployment_rate",
        title: "Taux de chômage",
        unit: "%",
        decimals: 1,
        source_name: "Bureau of Labor Statistics",
        source_url: "https://www.bls.gov/cps/",
        note: "Taux de chômage civil désaisonnalisé, publié chaque mois.",
        primary_label: "Chômage",
        secondary_label: None,
        quarterly: false,
    },
    EconomicIndicator {
        id: "nonfarm_payrolls",
        title: "Emploi non agricole",
        unit: "K",
        decimals: 0,
        source_name: "Bureau of Labor Statistics",
        source_url: "https://www.bls.gov/news.release/empsit.toc.htm",
        note: "Variation mensuelle de l’emploi salarié non agricole, exprimée en K. Les valeurs initiales ont été retranscrites depuis les captures Investing.com fournies; la référence statistique est le BLS Current Employment Statistics.",
        primary_label: "Variation mensuelle (K)",
        secondary_label: None,
        quarterly: false,
    },
    EconomicIndicator {
        id: "real_gdp_growth",
        title: "Croissance du PIB réel",
        unit: "% annualisé",
        decimals: 1,
        source_name: "Bureau of Economic Analysis",
        source_url: "https://www.bea.gov/data/gdp/gross-domestic-product",
        note: "Variation trimestrielle annualisée du PIB réel. Historique saisi depuis la colonne « Actuel » des captures fournies : dernière valeur publiée pour chaque trimestre, hors prévision et valeur précédente.",
        primary_label: "PIB réel",
        secondary_label: None,
        quarterly: true,
    },
    EconomicIndicator {
        id: "ism_manufacturing_pmi",
        title: "PMI manufacturier ISM",
        unit: "indice",
        decimals: 1,
        source_name: "Institute for Supply Management",
        source_url: "https://www.ismworld.org/supply-management-news-and-reports/reports/ism-pmi-reports/pmi/october/",
        note: "Indice de diffusion mensuel publié par l’ISM; au-dessus de 50, le secteur manufacturier est généralement en expansion. Les valeurs initiales viennent du tableau historique ISM à 12 mois.",
        primary_label: "PMI",
        secondary_label: None,
        quarterly: false,
    },
    EconomicIndicator {
        id: "ism_services_pmi",
        title: "PMI services ISM",
        unit: "indice",
        decimals: 1,
        source_name: "Institute for Supply Management",
        source_url: "https://www.ismworld.org/supply-management-news-and-reports/reports/ism-pmi-reports/services/september/",
        note: "Indice de diffusion mensuel publié par l’ISM; au-dessus de 50, le secteur des services est généralement en expansion. Les valeurs initiales viennent du tableau historique ISM à 12 mois.",
        primary_label: "PMI services",
        secondary_label: None,
        quarterly: false,
    },
];

#[derive(Serialize)]
struct EconomicObservationDraft {
    date: String,
    value: f64,
    value_secondary: Option<f64>,
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    error: String,
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

/// Onglet « Préparation hebdomadaire » : les liens du rituel de veille,
/// regroupés par moment de la semaine selon la méthode de la formation.
#[component]
fn WeeklySection() -> impl IntoView {
    let (groups, set_groups) = signal::<Option<Result<Vec<ResourceGroup>, String>>>(None);

    spawn_local(async move {
        set_groups.set(Some(fetch_veille_resources().await));
    });

    view! {
        <section class="veille-page">
            <div class="veille-intro">
                <div class="eyebrow"><span class="eyebrow-dot"></span> "PRÉPARATION HEBDOMADAIRE · VEILLE"</div>
                <h1>"La semaine se prépare la veille."</h1>
                <p>"Vendredi : relevé COT. Samedi — dimanche : un résumé par devise importante, puis une attente à court / moyen / long terme. Chaque matin vers 7 h : sentiment de risque, puis lectures. Tout est à un clic ci-dessous."</p>
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
                    <p class="veille-error">"Aucun groupe dans veille/links.json."</p>
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

    let load_entries = move |currency_code: String| {
        spawn_local(async move {
            match fetch_entries(&currency_code).await {
                Ok(items) => {
                    let still_selected = selected_currency.get().as_deref()
                        == Some(currency_code.as_str())
                        && active_section.get() == "currencies";
                    let still_unassigned =
                        currency_code == "UNASSIGNED" && active_section.get() == "journal";
                    if still_selected || still_unassigned {
                        set_entries.set(items);
                        set_save_status.set("BASE LOCALE · CONNECTÉE".to_string());
                        set_api_status.set(true);
                    }
                }
                Err(error) => {
                    let still_selected = selected_currency.get().as_deref()
                        == Some(currency_code.as_str())
                        && active_section.get() == "currencies";
                    let still_unassigned =
                        currency_code == "UNASSIGNED" && active_section.get() == "journal";
                    if still_selected || still_unassigned {
                        set_save_status.set(format!("API INDISPONIBLE · {error}"));
                        set_api_status.set(false);
                    }
                }
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

    let open_currency = move |code: &'static str| {
        set_active_section.set("currencies".to_string());
        set_selected_currency.set(Some(code.to_string()));
        set_editor_open.set(false);
        set_search.set(String::new());
        set_entries.set(Vec::new());
        load_entries(code.to_string());
    };

    let open_unassigned = move |_| {
        set_active_section.set("journal".to_string());
        set_selected_currency.set(None);
        set_editor_open.set(false);
        set_search.set(String::new());
        set_entries.set(Vec::new());
        load_entries("UNASSIGNED".to_string());
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
    let filtered_entries = move || {
        let query = search.get().trim().to_lowercase();
        entries
            .get()
            .into_iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.title.to_lowercase().contains(&query)
                    || entry.content.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>()
    };
    let section_title = move || match active_section.get().as_str() {
        "overview" => "Vue générale",
        "journal" => "Notes à classer",
        "currencies" => selected_currency
            .get()
            .and_then(|code| currency_info(&code))
            .map(|currency| currency.code)
            .unwrap_or("Dossiers de devises"),
        "weekly" => "Préparation hebdomadaire",
        "events" => "Actualités & événements",
        "pairs" => "Analyse des paires",
        "reviews" => "Bilans",
        _ => "Journal de recherche",
    };

    view! {
        <div class="app-shell" class:sidebar_hidden=move || !sidebar_visible.get()>
            <aside class="sidebar">
                <a class="brand-lockup" href="#" on:click=move |_| {
                    set_active_section.set("journal".to_string());
                    set_editor_open.set(false);
                    set_selected_currency.set(None);
                    set_search.set(String::new());
                    set_entries.set(Vec::new());
                    load_entries("UNASSIGNED".to_string());
                }>
                    <div class="brand-mark">"M"</div>
                    <div class="brand-copy">
                        <strong>"MACRO JOURNAL"</strong>
                        <span>"CARNET DE RECHERCHE"</span>
                    </div>
                </a>

                <div class="sidebar-section-label">"ESPACE DE TRAVAIL"</div>
                <nav class="main-nav" aria-label="Navigation principale">
                    <button class="nav-item" class:active=move || active_section.get() == "overview" on:click=move |_| {
                        set_active_section.set("overview".to_string());
                        set_selected_currency.set(None);
                        set_editor_open.set(false);
                    }>
                        <span class="nav-icon">"⌂"</span><span>"Vue générale"</span>
                    </button>
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
                    <button class="nav-item" class:active=move || active_section.get() == "events" on:click=move |_| set_active_section.set("events".to_string())>
                        <span class="nav-icon">"◷"</span><span>"Actualités & événements"</span>
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
                                            placeholder="Donnez un titre à votre observation"
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
                                                <span>"Écrivez librement — les retours à la ligne sont conservés."</span>
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

                                <section class="economic-data-section">
                                    <div class="economic-data-heading">
                                        <div>
                                            <div class="eyebrow">"FONDAMENTAUX · SOURCES PRIMAIRES"</div>
                                            <h2>"Données économiques"</h2>
                                        </div>
                                        <span class="cot-indicator-label">"DONNÉES RÉALISÉES · SANS CONSENSUS"</span>
                                    </div>
                                    <p class="economic-data-intro">
                                        {format!("Historique {} conservé dans un CSV local, sans actualisation externe. Ouvrez un indicateur pour choisir sa période et son affichage, puis saisir une nouvelle donnée. Les séries absentes ou partielles ne sont pas estimées.", currency.code)}
                                    </p>
                                    {if matches!(currency.code, "CHF" | "NZD") {
                                        view! {
                                            <p class="economic-data-intro">
                                                "PMI S&P Global non collecté pour cette devise : aucun substitut n’est présenté."
                                            </p>
                                        }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                    <EconomicDashboard currency_code=currency.code/>
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
                                                <h3>{move || if entries.get().is_empty() { format!("Le dossier {} est prêt.", currency.code) } else { "Aucune note ne correspond à cette recherche.".to_string() }}</h3>
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
                    } else if active_section.get() == "currencies"
                        || active_section.get() == "overview"
                    {
                        view! {
                            <section class="currency-index-page">
                                <div class="currency-index-intro">
                                    <div class="eyebrow"><span class="eyebrow-dot"></span> "RECHERCHE FONDAMENTALE · VOS DOSSIERS"</div>
                                    <h1>"Une devise, un espace."</h1>
                                    <p>"Chaque économie a son propre dossier. Vos observations restent cloisonnées ; les comparaisons entre devises viendront plus tard."</p>
                                </div>

                                <div class="currency-group-heading">
                                    <div>
                                        <div class="eyebrow">"LES 8 DEVISES MAJEURES"</div>
                                        <h2>"Choisissez votre dossier"</h2>
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
                                    <p>"La Chine, notamment pour l’AUD et le NZD."</p>
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
                                    <span><strong>"Notes à classer"</strong><small>"Retrouver les anciennes notes qui n’étaient pas associées à une devise."</small></span>
                                    <span class="currency-card-arrow">"→"</span>
                                </button>
                            </section>
                        }.into_any()
                    } else if active_section.get() == "journal" {
                        view! {
                            <section class="dashboard-page">
                                <div class="welcome-banner">
                                    <div class="welcome-copy">
                                        <div class="eyebrow"><span class="eyebrow-dot"></span> "ARCHIVE HISTORIQUE · SANS DEVISE ASSOCIÉE"</div>
                                        <h1>"Notes à classer."<br/><span>"Votre historique est conservé."</span></h1>
                                        <p>"Ces notes précèdent le classement par devise. Elles restent ici, intactes ; toute nouvelle analyse commence dans son dossier dédié."</p>
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
                                        <div class="stat-label"><span class="stat-icon">"▤"</span> "DANS CETTE ARCHIVE"</div>
                                        <div class="stat-value">{move || entries.get().len()}</div>
                                        <div class="stat-detail">"notes sans devise associée"</div>
                                    </article>
                                    <article class="stat-card">
                                        <div class="stat-label"><span class="stat-icon stat-icon-sand">"◷"</span> "DERNIÈRE ACTIVITÉ"</div>
                                        <div class="stat-value stat-date">{move || {
                                            entries.get().first()
                                                .map(|entry| format_date(&entry.updated_at))
                                                .unwrap_or_else(|| "—".to_string())
                                        }}</div>
                                        <div class="stat-detail">{move || if entries.get().is_empty() { "Votre première note vous attend" } else { "Mise à jour la plus récente" }}</div>
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
                                                <div class="eyebrow">"VOTRE HISTORIQUE"</div>
                                                <h2>"Anciennes notes"</h2>
                                                <p>"Aucune note n’est réaffectée automatiquement à une devise."</p>
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

                                        <Show
                                            when=move || !filtered_entries().is_empty()
                                            fallback=move || view! {
                                                <div class="empty-journal">
                                                    <div class="empty-journal-icon">{move || if entries.get().is_empty() { "✳" } else { "⌕" }}</div>
                                                    <h3>{move || if entries.get().is_empty() { "Aucune ancienne note à classer." } else { "Aucune note ne correspond à cette recherche." }}</h3>
                                                    <p>{move || if entries.get().is_empty() { "Les notes créées à partir de maintenant seront enregistrées dans le dossier de la devise concernée.".to_string() } else { "Essayez un autre mot-clé ou effacez votre recherche.".to_string() }}</p>
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
                                                                    <span class="research-card-type"><span>"✳"</span> "ANALYSE"</span>
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
                                            <h2>"Une conviction se construit."</h2>
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
                            <WeeklySection/>
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

#[component]
fn EconomicDashboard(currency_code: &'static str) -> impl IntoView {
    let (observations, set_observations) =
        signal::<Option<Result<Vec<EconomicObservation>, String>>>(None);
    let (selected_indicator, set_selected_indicator) = signal::<Option<String>>(None);
    let (years, set_years) = signal(3_u8);
    let (chart_type, set_chart_type) = signal("line".to_string());
    let (date, set_date) = signal(String::new());
    let (value, set_value) = signal(String::new());
    let (secondary_value, set_secondary_value) = signal(String::new());
    let (save_status, set_save_status) = signal(String::new());

    let fetch_currency = currency_code;
    spawn_local(async move {
        set_observations.set(Some(fetch_economic_observations(fetch_currency).await));
    });

    let save_currency = currency_code;
    let save_observation = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let Some(series_id) = selected_indicator.get() else {
            return;
        };
        let indicator =
            economic_indicator(save_currency, &series_id).expect("selected indicator is known");
        let parsed_value = value.get().trim().parse::<f64>();
        let Ok(parsed_value) = parsed_value else {
            set_save_status.set("Saisissez une valeur numérique valide.".to_string());
            return;
        };
        let parsed_secondary = if indicator.secondary_label.is_some() {
            match secondary_value.get().trim().parse::<f64>() {
                Ok(number) => Some(number),
                Err(_) => {
                    set_save_status.set("Saisissez également la seconde valeur.".to_string());
                    return;
                }
            }
        } else {
            None
        };
        let draft = EconomicObservationDraft {
            date: date.get(),
            value: parsed_value,
            value_secondary: parsed_secondary,
        };
        set_save_status.set("Enregistrement…".to_string());

        let currency_code = save_currency;
        spawn_local(async move {
            match add_economic_observation(currency_code, &series_id, draft).await {
                Ok(saved) => {
                    set_observations.update(|current| {
                        if let Some(Ok(rows)) = current {
                            rows.retain(|row| {
                                row.series_id != saved.series_id || row.date != saved.date
                            });
                            rows.push(saved);
                            rows.sort_by(|left, right| {
                                left.series_id
                                    .cmp(&right.series_id)
                                    .then_with(|| left.date.cmp(&right.date))
                            });
                        }
                    });
                    set_value.set(String::new());
                    set_secondary_value.set(String::new());
                    set_save_status.set("Donnée enregistrée dans le CSV local.".to_string());
                }
                Err(error) => {
                    set_save_status.set(format!("Échec de l’enregistrement : {error}"));
                }
            }
        });
    };

    view! {
        <div class="economic-dashboard">
            {move || match observations.get() {
                None => view! {
                    <p class="cot-state">{format!("Chargement de l’historique {} local…", currency_code)}</p>
                }.into_any(),
                Some(Err(error)) => view! {
                    <p class="cot-state cot-state-error" role="alert">
                        "Impossible de charger l’historique économique local : " {error}
                    </p>
                }.into_any(),
                Some(Ok(rows)) => {
                    match selected_indicator.get() {
                        None => view! {
                            <div class="economic-index">
                                {economic_indicators(currency_code).into_iter().map(|indicator| {
                                    let series_rows = rows.iter()
                                        .filter(|row| row.series_id == indicator.id)
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    let latest = series_rows.last().cloned();
                                    view! {
                                        <button
                                            class="economic-index-card"
                                            on:click=move |_| {
                                                set_selected_indicator.set(Some(indicator.id.to_string()));
                                                set_years.set(3);
                                                set_chart_type.set("line".to_string());
                                                set_save_status.set(String::new());
                                            }
                                        >
                                            <span class="economic-index-card-top">
                                                <strong>{indicator.title}</strong>
                                                <span aria-hidden="true">"↗"</span>
                                            </span>
                                            {match latest {
                                                Some(latest) => view! {
                                                    <span class="economic-index-value">
                                                        {economic_observation_label(indicator, &latest)}
                                                    </span>
                                                    <span class="economic-index-meta">
                                                        {economic_observation_meta(indicator, &latest)}
                                                    </span>
                                                }.into_any(),
                                                None => view! {
                                                    <span class="economic-index-value economic-index-empty">
                                                        "Aucune observation dans ce CSV"
                                                    </span>
                                                    <span class="economic-index-meta">
                                                        {format!("{} · saisie manuelle", indicator.unit)}
                                                    </span>
                                                }.into_any(),
                                            }}
                                        </button>
                                    }
                                }).collect_view()}
                            </div>
                        }.into_any(),
                        Some(id) => match economic_indicator(currency_code, &id) {
                            Some(indicator) => {
                                let mut series_rows = rows.into_iter()
                                    .filter(|row| row.series_id == indicator.id)
                                    .collect::<Vec<_>>();
                                series_rows.sort_by(|left, right| left.date.cmp(&right.date));
                                let latest = series_rows.last().cloned();
                                view! {
                                    <section class="economic-detail">
                                        <button
                                            class="back-link economic-back"
                                            on:click=move |_| {
                                                set_selected_indicator.set(None);
                                                set_save_status.set(String::new());
                                            }
                                        >
                                            <span aria-hidden="true">"←"</span>
                                            {format!("Tous les indicateurs {}", currency_code)}
                                        </button>
                                        <div class="economic-detail-heading">
                                            <div>
                                                <div class="eyebrow">{format!("{} · DONNÉES RÉALISÉES", currency_code)}</div>
                                                <h3>{indicator.title}</h3>
                                                <p>{indicator.note}</p>
                                            </div>
                                            <span class="cot-indicator-label">{format!("HISTORIQUE LOCAL · {} OBS.", series_rows.len())}</span>
                                        </div>
                                        <div class="economic-latest">
                                            {match latest.as_ref() {
                                                Some(latest) => view! {
                                                    <div class="economic-latest-values">
                                                        <div>
                                                            <span>{indicator.primary_label}</span>
                                                            <strong>{format_economic_value(latest.value, indicator)}</strong>
                                                        </div>
                                                        {indicator.secondary_label.map(|label| view! {
                                                            <div>
                                                                <span>{label}</span>
                                                                <strong>
                                                                    {latest.value_secondary
                                                                        .map(|number| format_economic_value(number, indicator))
                                                                        .unwrap_or_else(|| "—".to_string())}
                                                                </strong>
                                                            </div>
                                                        })}
                                                    </div>
                                                    <span class="economic-latest-date">
                                                        {format!("Période de la donnée · {}", economic_period_label(indicator, &latest.date))}
                                                    </span>
                                                }.into_any(),
                                                None => view! {
                                                    <p>"Aucune observation locale pour cet indicateur. Ajoutez une publication ci-dessous."</p>
                                                }.into_any(),
                                            }}
                                        </div>
                                        <div class="economic-chart-tools">
                                            <div class="economic-control-group" aria-label="Période affichée">
                                                <span>"PÉRIODE"</span>
                                                {[1_u8, 2, 3].into_iter().map(|period| view! {
                                                    <button
                                                        class="economic-control-button"
                                                        class:active=move || years.get() == period
                                                        aria-pressed=move || years.get() == period
                                                        on:click=move |_| set_years.set(period)
                                                    >
                                                        {period} " an" {if period > 1 { "s" } else { "" }}
                                                    </button>
                                                }).collect_view()}
                                            </div>
                                            <div class="economic-control-group" aria-label="Type de graphique">
                                                <span>"GRAPHIQUE"</span>
                                                <button
                                                    class="economic-control-button"
                                                    class:active=move || chart_type.get() == "line"
                                                    aria-pressed=move || chart_type.get() == "line"
                                                    on:click=move |_| set_chart_type.set("line".to_string())
                                                >"Courbe"</button>
                                                <button
                                                    class="economic-control-button"
                                                    class:active=move || chart_type.get() == "bar"
                                                    aria-pressed=move || chart_type.get() == "bar"
                                                    on:click=move |_| set_chart_type.set("bar".to_string())
                                                >"Histogramme"</button>
                                            </div>
                                        </div>
                                        {move || {
                                            let svg = render_economic_chart(
                                                &series_rows,
                                                years.get(),
                                                &chart_type.get(),
                                                indicator,
                                            );
                                            if svg.is_empty() {
                                                view! {
                                                    <p class="economic-chart-empty">
                                                        "Aucune donnée disponible pour cette période."
                                                    </p>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <div class="economic-chart-wrap" inner_html=svg></div>
                                                }.into_any()
                                            }
                                        }}
                                        <div class="economic-detail-footer">
                                            <span>{format!("Unité : {}", indicator.unit)}</span>
                                            <a
                                                class="economic-source-link"
                                                href=indicator.source_url
                                                target="_blank"
                                                rel="noopener noreferrer"
                                            >
                                                {format!("Source : {}", indicator.source_name)} <span>"↗"</span>
                                            </a>
                                        </div>
                                        <details class="economic-add-form">
                                            <summary><span>"＋"</span> "Ajouter une publication"</summary>
                                            <form on:submit=save_observation>
                                                <div class="economic-form-grid">
                                                    <label>
                                                        <span>"PÉRIODE DE LA DONNÉE"</span>
                                                        <input
                                                            type="date"
                                                            required
                                                            prop:value=move || date.get()
                                                            on:input=move |event| set_date.set(event_target_value(&event))
                                                        />
                                                    </label>
                                                    <label>
                                                        <span>{indicator.primary_label}</span>
                                                        <input
                                                            type="number"
                                                            step=if indicator.decimals == 0 { "1" } else { "any" }
                                                            required
                                                            prop:value=move || value.get()
                                                            on:input=move |event| set_value.set(event_target_value(&event))
                                                        />
                                                    </label>
                                                    {indicator.secondary_label.map(|label| view! {
                                                        <label>
                                                            <span>{label}</span>
                                                            <input
                                                                type="number"
                                                                step=if indicator.decimals == 0 { "1" } else { "any" }
                                                                required
                                                                prop:value=move || secondary_value.get()
                                                                on:input=move |event| set_secondary_value.set(event_target_value(&event))
                                                            />
                                                        </label>
                                                    })}
                                                    <button class="button button-primary economic-save-button" type="submit">
                                                        "Enregistrer la publication"
                                                    </button>
                                                </div>
                                                <p class="economic-form-help">
                                                    {if indicator.quarterly {
                                                        "Datez la donnée au premier jour du trimestre. "
                                                    } else {
                                                        "Datez la donnée au premier jour du mois. "
                                                    }}
                                                    "Une date déjà présente sera remplacée pour permettre de corriger une révision."
                                                </p>
                                                <p class="economic-form-status" role="status">{move || save_status.get()}</p>
                                            </form>
                                        </details>
                                    </section>
                                }.into_any()
                            }
                            None => view! {
                                <p class="cot-state cot-state-error" role="alert">
                                    "Indicateur inconnu pour cette devise."
                                </p>
                            }.into_any(),
                        }
                    }
                }
            }}
        </div>
    }
}

fn economic_indicators(currency_code: &str) -> Vec<EconomicIndicator> {
    let ids: &[&str] = match currency_code {
        "USD" => return ECONOMIC_INDICATORS.to_vec(),
        "EUR" => &[
            "policy_rate",
            "hicp_headline_yoy",
            "hicp_core_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
            "sp_global_manufacturing_pmi",
            "sp_global_services_pmi",
        ],
        "GBP" | "JPY" => &[
            "policy_rate",
            "cpi_headline_yoy",
            "cpi_core_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
            "sp_global_manufacturing_pmi",
            "sp_global_services_pmi",
        ],
        "CHF" => &[
            "policy_rate",
            "cpi_headline_yoy",
            "cpi_core_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
        ],
        "CAD" => &[
            "policy_rate",
            "cpi_common",
            "cpi_median_yoy",
            "cpi_trimmed_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
            "sp_global_manufacturing_pmi",
            "sp_global_services_pmi",
        ],
        "AUD" => &[
            "policy_rate",
            "cpi_trimmed_yoy",
            "cpi_weighted_median_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
            "sp_global_manufacturing_pmi",
            "sp_global_services_pmi",
        ],
        "NZD" => &[
            "policy_rate",
            "cpi_headline_yoy",
            "cpi_core_proxy_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
        ],
        "CNY" => &[
            "policy_rate",
            "cpi_headline_yoy",
            "cpi_core_yoy",
            "ppi_yoy",
            "unemployment_rate",
            "gdp_qoq",
            "gdp_yoy",
            "ratingdog_manufacturing_pmi",
            "ratingdog_services_pmi",
        ],
        _ => return Vec::new(),
    };
    ids.iter()
        .filter_map(|id| economic_indicator(currency_code, id))
        .collect()
}

fn economic_indicator(currency_code: &str, id: &str) -> Option<EconomicIndicator> {
    if currency_code == "USD" {
        return ECONOMIC_INDICATORS
            .iter()
            .find(|indicator| indicator.id == id)
            .copied();
    }

    let title = match id {
        "policy_rate" => "Taux directeur",
        "hicp_headline_yoy" => "Inflation HICP harmonisée · globale",
        "hicp_core_yoy" => "Inflation HICP harmonisée · sous-jacente",
        "cpi_headline_yoy" => "Inflation CPI · globale",
        "cpi_core_yoy" => "Inflation CPI · sous-jacente",
        "cpi_common" => "Inflation CPI commune",
        "cpi_median_yoy" => "Inflation CPI médiane",
        "cpi_trimmed_yoy" if currency_code == "AUD" => "Inflation CPI trimmed mean",
        "cpi_trimmed_yoy" => "Inflation CPI tronquée",
        "cpi_weighted_median_yoy" => "Inflation CPI weighted median",
        "cpi_core_proxy_yoy" => "Inflation CPI · proxy sous-jacent",
        "ppi_yoy" => "Inflation PPI",
        "unemployment_rate" => "Taux de chômage",
        "gdp_qoq" => "PIB réel · croissance trimestrielle",
        "gdp_yoy" => "PIB réel · croissance annuelle",
        "sp_global_manufacturing_pmi" => "PMI manufacturier S&P Global",
        "sp_global_services_pmi" => "PMI services S&P Global",
        "ratingdog_manufacturing_pmi" => "PMI manufacturier RatingDog",
        "ratingdog_services_pmi" => "PMI services RatingDog",
        _ => return None,
    };
    let (source_name, source_url) = economic_source(currency_code, id)?;
    let quarterly = matches!(id, "gdp_qoq" | "gdp_yoy")
        || (currency_code == "AUD"
            && matches!(
                id,
                "cpi_trimmed_yoy" | "cpi_weighted_median_yoy" | "ppi_yoy"
            ))
        || (currency_code == "CHF" && id == "unemployment_rate")
        || (currency_code == "NZD"
            && matches!(
                id,
                "cpi_headline_yoy" | "cpi_core_proxy_yoy" | "ppi_yoy" | "unemployment_rate"
            ));
    let unit = match id {
        "policy_rate" | "unemployment_rate" => "%",
        "gdp_qoq" => "% QoQ",
        "gdp_yoy" => "% YoY",
        value if value.ends_with("_pmi") || value.ends_with("_services_pmi") => "indice",
        _ => "% sur 12 mois",
    };
    let primary_label = match id {
        "policy_rate" => "Taux directeur",
        "hicp_headline_yoy" | "cpi_headline_yoy" => "Inflation globale",
        "hicp_core_yoy" | "cpi_core_yoy" => "Inflation sous-jacente",
        "cpi_common" => "CPI common",
        "cpi_median_yoy" => "CPI median",
        "cpi_trimmed_yoy" => "CPI trimmed mean",
        "cpi_weighted_median_yoy" => "CPI weighted median",
        "cpi_core_proxy_yoy" => "Proxy sous-jacent",
        "ppi_yoy" => "PPI",
        "unemployment_rate" => "Chômage",
        "gdp_qoq" => "Croissance QoQ",
        "gdp_yoy" => "Croissance YoY",
        "sp_global_manufacturing_pmi" | "ratingdog_manufacturing_pmi" => "PMI",
        _ => "PMI services",
    };
    let note = match (currency_code, id) {
        ("EUR", "hicp_headline_yoy" | "hicp_core_yoy") => {
            "Indice des prix à la consommation harmonisé (IPCH/HICP) de la zone euro; mesures harmonisées et comparables entre pays."
        }
        ("CHF", "cpi_headline_yoy" | "cpi_core_yoy") => {
            "IPC national suisse (LIK), et non HICP. La mesure sous-jacente suit la définition publiée par l’Office fédéral de la statistique."
        }
        ("CAD", "cpi_common") => {
            "Mesure CPI-common de la Banque du Canada; mesure de tendance sous-jacente distincte des CPI trimmed et median."
        }
        ("CAD", "cpi_trimmed_yoy") => {
            "CPI-trim de la Banque du Canada; mesure sous-jacente qui réduit l’influence des variations extrêmes."
        }
        ("CAD", "cpi_median_yoy") => {
            "CPI-median de la Banque du Canada; variation de prix pondérée au 50e percentile."
        }
        ("AUD", "cpi_trimmed_yoy") => {
            "Trimmed mean CPI publiée par l’ABS. Publication trimestrielle; les dates suivent le premier jour du trimestre."
        }
        ("AUD", "cpi_weighted_median_yoy") => {
            "Weighted median CPI publiée par l’ABS. Publication trimestrielle; les dates suivent le premier jour du trimestre."
        }
        ("NZD", "cpi_core_proxy_yoy") => {
            "Proxy construit à partir de la mesure de prix sous-jacente documentée dans les données; ce n’est pas une série officielle intitulée « core CPI »."
        }
        ("CNY", "ratingdog_manufacturing_pmi" | "ratingdog_services_pmi") => {
            "PMI final publié par S&P Global sous la marque RatingDog (anciennement Caixin) depuis août 2025. Au-dessus de 50, l’activité est généralement en expansion."
        }
        (_, "policy_rate") => {
            "Taux directeur en vigueur au premier jour du mois; aucune observation quotidienne n’est nécessaire pour l’historique mensuel."
        }
        (_, "gdp_qoq") => {
            "Variation trimestrielle du PIB réel par rapport au trimestre précédent, non annualisée. Date = premier jour du trimestre."
        }
        (_, "gdp_yoy") => {
            "Variation du PIB réel sur un an. Date = premier jour du trimestre."
        }
        (_, "unemployment_rate") if quarterly => {
            "Taux de chômage publié trimestriellement; date = premier jour du trimestre."
        }
        (_, "unemployment_rate") => "Taux de chômage publié mensuellement.",
        (_, value) if value.ends_with("_pmi") || value.ends_with("_services_pmi") => {
            "Indice PMI final mensuel publié par S&P Global. Au-dessus de 50, l’activité est généralement en expansion; les périodes absentes ne sont pas estimées."
        }
        (_, "ppi_yoy") if quarterly => {
            "Variation annuelle de l’indice des prix à la production, publiée trimestriellement; date = premier jour du trimestre."
        }
        (_, "ppi_yoy") => "Variation annuelle des prix à la production.",
        _ => "Variation annuelle des prix à la consommation.",
    };
    let indicator_id = match id {
        "policy_rate" => "policy_rate",
        "hicp_headline_yoy" => "hicp_headline_yoy",
        "hicp_core_yoy" => "hicp_core_yoy",
        "cpi_headline_yoy" => "cpi_headline_yoy",
        "cpi_core_yoy" => "cpi_core_yoy",
        "cpi_common" => "cpi_common",
        "cpi_median_yoy" => "cpi_median_yoy",
        "cpi_trimmed_yoy" => "cpi_trimmed_yoy",
        "cpi_weighted_median_yoy" => "cpi_weighted_median_yoy",
        "cpi_core_proxy_yoy" => "cpi_core_proxy_yoy",
        "ppi_yoy" => "ppi_yoy",
        "unemployment_rate" => "unemployment_rate",
        "gdp_qoq" => "gdp_qoq",
        "gdp_yoy" => "gdp_yoy",
        "sp_global_manufacturing_pmi" => "sp_global_manufacturing_pmi",
        "sp_global_services_pmi" => "sp_global_services_pmi",
        "ratingdog_manufacturing_pmi" => "ratingdog_manufacturing_pmi",
        "ratingdog_services_pmi" => "ratingdog_services_pmi",
        _ => return None,
    };
    Some(EconomicIndicator {
        id: indicator_id,
        title,
        unit,
        decimals: if id == "policy_rate" { 2 } else { 1 },
        source_name,
        source_url,
        note,
        primary_label,
        secondary_label: None,
        quarterly,
    })
}

fn economic_source(currency_code: &str, id: &str) -> Option<(&'static str, &'static str)> {
    if id.ends_with("_pmi") || id.ends_with("_services_pmi") {
        return Some(if currency_code == "CNY" {
            (
                "S&P Global · RatingDog",
                "https://www.pmi.spglobal.com/Public/Release/PressReleases",
            )
        } else {
            (
                "S&P Global",
                "https://www.pmi.spglobal.com/Public/Release/PressReleases",
            )
        });
    }
    Some(match currency_code {
        "EUR" if id == "policy_rate" => (
            "Banque centrale européenne",
            "https://www.ecb.europa.eu/stats/policy_and_exchange_rates/key_ecb_interest_rates/html/index.en.html",
        ),
        "EUR" => (
            "Eurostat",
            "https://ec.europa.eu/eurostat/web/main/data/database",
        ),
        "GBP" if id == "policy_rate" => (
            "Bank of England",
            "https://www.bankofengland.co.uk/boeapps/database/Bank-Rate.asp",
        ),
        "GBP" => (
            "Office for National Statistics",
            "https://www.ons.gov.uk/economy",
        ),
        "JPY" if id == "policy_rate" => (
            "Bank of Japan",
            "https://www.boj.or.jp/en/mopo/mpmdeci/index.htm",
        ),
        "JPY" if id == "ppi_yoy" => (
            "Bank of Japan",
            "https://www.boj.or.jp/en/statistics/pi/index.htm",
        ),
        "JPY" if id == "gdp_qoq" || id == "gdp_yoy" => (
            "Cabinet Office of Japan",
            "https://www.esri.cao.go.jp/en/sna/menu.html",
        ),
        "JPY" => (
            "Statistics Bureau of Japan",
            "https://www.stat.go.jp/english/data/index.html",
        ),
        "CHF" if id == "policy_rate" => (
            "Banque nationale suisse",
            "https://www.snb.ch/en/the-snb/mandates-goals/monetary-policy",
        ),
        "CHF" if id == "gdp_qoq" || id == "gdp_yoy" => (
            "SECO",
            "https://www.seco.admin.ch/seco/en/home/wirtschaftslage---wirtschaftspolitik/Wirtschaftslage/bip-quartalsschaetzungen-.html",
        ),
        "CHF" if id == "ppi_yoy" => (
            "Eurostat · source primaire OFS",
            "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data/sts_inpp_m?format=JSON&lang=EN&geo=CH&nace_r2=B-D&s_adj=NSA&unit=PCH_SM&sinceTimePeriod=2023-11",
        ),
        "CHF" if id == "unemployment_rate" => (
            "OFS / OECD via FRED",
            "https://fred.stlouisfed.org/graph/fredgraph.csv?id=LRHUTTTTCHQ156S",
        ),
        "CHF" => (
            "Office fédéral de la statistique",
            "https://www.bfs.admin.ch/bfs/en/home/statistics/prices/consumer-price-index.html",
        ),
        "CAD" if id == "policy_rate" => (
            "Banque du Canada",
            "https://www.bankofcanada.ca/core-functions/monetary-policy/key-interest-rate/",
        ),
        "CAD" if id.starts_with("cpi_") => (
            "Banque du Canada",
            "https://www.bankofcanada.ca/rates/price-indexes/cpi/",
        ),
        "CAD" => (
            "Statistique Canada",
            "https://www.statcan.gc.ca/en/subjects-start/prices_and_price_indexes",
        ),
        "AUD" if id == "policy_rate" => (
            "Reserve Bank of Australia",
            "https://www.rba.gov.au/statistics/cash-rate/",
        ),
        "AUD" if id.starts_with("cpi_") => (
            "Reserve Bank of Australia",
            "https://www.rba.gov.au/statistics/tables/",
        ),
        "AUD" => (
            "Australian Bureau of Statistics",
            "https://www.abs.gov.au/statistics",
        ),
        "NZD" if id == "policy_rate" => (
            "Reserve Bank of New Zealand",
            "https://www.rbnz.govt.nz/monetary-policy/about-monetary-policy",
        ),
        "NZD" => (
            "Stats NZ",
            "https://www.stats.govt.nz/topics/economy/",
        ),
        "CNY" if id == "policy_rate" => (
            "People’s Bank of China",
            "http://www.pbc.gov.cn/en/3688229/3688335/index.html",
        ),
        "CNY" => (
            "National Bureau of Statistics of China",
            "https://www.stats.gov.cn/english/",
        ),
        _ => return None,
    })
}

fn economic_observation_label(
    indicator: EconomicIndicator,
    observation: &EconomicObservation,
) -> String {
    let primary = format_economic_value(observation.value, indicator);
    match (indicator.id, observation.value_secondary) {
        ("fed_target_range", Some(upper)) => {
            format!("{}–{}", primary, format_economic_value(upper, indicator))
        }
        ("pce_inflation", _) => primary,
        _ => primary,
    }
}

fn economic_observation_meta(
    indicator: EconomicIndicator,
    observation: &EconomicObservation,
) -> String {
    let date = format!(
        "Dernière donnée · {}",
        economic_period_label(indicator, &observation.date)
    );
    match (indicator.id, observation.value_secondary) {
        ("fed_target_range", _) => format!("Fourchette cible · {date}"),
        (_, Some(secondary)) => format!(
            "{} {} · {date}",
            indicator.secondary_label.unwrap_or("Valeur secondaire"),
            format_economic_value(secondary, indicator)
        ),
        _ => date,
    }
}

fn format_economic_value(value: f64, indicator: EconomicIndicator) -> String {
    let number = if indicator.id == "nonfarm_payrolls" {
        format!("{value:.0}")
    } else if indicator.id == "fed_target_range" {
        format!("{value:.precision$}", precision = indicator.decimals)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .replace('.', ",")
    } else {
        format!("{value:.precision$}", precision = indicator.decimals).replace('.', ",")
    };
    if indicator.id == "nonfarm_payrolls" {
        format!("{number}K")
    } else {
        format!("{number} {}", indicator.unit)
    }
}

fn render_economic_chart(
    observations: &[EconomicObservation],
    years: u8,
    chart_type: &str,
    indicator: EconomicIndicator,
) -> String {
    let Some(latest) = observations.last() else {
        return String::new();
    };
    let cutoff = month_index(&latest.date) - i32::from(years) * 12;
    let visible = observations
        .iter()
        .filter(|row| month_index(&row.date) >= cutoff)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return String::new();
    }

    const LEFT: f64 = 65.0;
    const RIGHT: f64 = 875.0;
    const TOP: f64 = 22.0;
    const BOTTOM: f64 = 258.0;
    let mut values = visible
        .iter()
        .flat_map(|row| std::iter::once(row.value).chain(row.value_secondary));
    let first = values.next().unwrap_or_default();
    let (minimum, maximum) = values.fold((first, first), |(min, max), value| {
        (min.min(value), max.max(value))
    });
    let padding = ((maximum - minimum) * 0.12)
        .max(maximum.abs().max(minimum.abs()) * 0.025)
        .max(0.1);
    let low = minimum - padding;
    let high = maximum + padding;
    let y = |value: f64| BOTTOM - ((value - low) / (high - low)) * (BOTTOM - TOP);
    let x = |index: usize| {
        if visible.len() == 1 {
            (LEFT + RIGHT) / 2.0
        } else {
            LEFT + index as f64 / (visible.len() - 1) as f64 * (RIGHT - LEFT)
        }
    };

    let mut svg = format!(
        r##"<svg class="economic-svg" viewBox="0 0 920 320" role="img" aria-label="{} sur {} {}"><rect width="920" height="320" rx="8" fill="#0b110e"/>"##,
        indicator.title,
        years,
        if years == 1 { "an" } else { "ans" }
    );
    for index in 0..=4 {
        let value = high - (high - low) * index as f64 / 4.0;
        let y_position = y(value);
        svg.push_str(&format!(
            r##"<line x1="{LEFT}" y1="{y_position:.1}" x2="{RIGHT}" y2="{y_position:.1}" stroke="#26332b"/><text x="54" y="{:.1}" text-anchor="end" fill="#91a196" font-size="11">{}</text>"##,
            y_position + 4.0,
            format!(
                "{value:.precision$}",
                precision = indicator.decimals
            )
            .replace('.', ",")
        ));
    }

    if chart_type == "bar" {
        let slot_width = (RIGHT - LEFT) / visible.len() as f64;
        let dual = indicator.secondary_label.is_some();
        let bar_width = (slot_width * if dual { 0.32 } else { 0.58 }).clamp(3.0, 24.0);
        for (index, row) in visible.iter().enumerate() {
            let center = LEFT + (index as f64 + 0.5) * slot_width;
            let values = [Some(row.value), row.value_secondary];
            for (series_index, value) in values.into_iter().enumerate() {
                if let Some(value) = value {
                    let baseline = y(0.0_f64.clamp(low, high));
                    let top = y(value).min(baseline);
                    let height = (baseline - y(value)).abs().max(1.0);
                    let offset = if dual {
                        (series_index as f64 - 0.5) * bar_width
                    } else {
                        0.0
                    };
                    let color = if series_index == 0 {
                        "#c8a9ff"
                    } else {
                        "#56cfe1"
                    };
                    svg.push_str(&format!(
                        r##"<rect x="{:.1}" y="{top:.1}" width="{bar_width:.1}" height="{height:.1}" rx="2" fill="{color}"><title>{} · {}</title></rect>"##,
                        center + offset - bar_width / 2.0,
                        row.date,
                        format_economic_value(value, indicator)
                    ));
                }
            }
        }
    } else {
        for (series_index, color) in [(0, "#c8a9ff"), (1, "#56cfe1")] {
            if series_index == 1 && indicator.secondary_label.is_none() {
                continue;
            }
            let points = visible
                .iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    let value = if series_index == 0 {
                        Some(row.value)
                    } else {
                        row.value_secondary
                    }?;
                    Some((index, row, value))
                })
                .collect::<Vec<_>>();
            if points.is_empty() {
                continue;
            }
            let path = points
                .iter()
                .enumerate()
                .map(|(point_index, (index, row, value))| {
                    let has_gap = point_index > 0
                        && points.get(point_index - 1).is_some_and(|(_, previous, _)| {
                            month_index(&row.date) - month_index(&previous.date)
                                > if indicator.quarterly { 3 } else { 1 }
                        });
                    format!(
                        "{} {:.1},{:.1}",
                        if point_index == 0 || has_gap {
                            "M"
                        } else {
                            "L"
                        },
                        x(*index),
                        y(*value)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            svg.push_str(&format!(
                r##"<path d="{path}" fill="none" stroke="{color}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>"##
            ));
            for (index, row, value) in points {
                svg.push_str(&format!(
                    r##"<circle cx="{:.1}" cy="{:.1}" r="3" fill="{color}"><title>{} · {}</title></circle>"##,
                    x(index),
                    y(value),
                    row.date,
                    format_economic_value(value, indicator)
                ));
            }
        }
    }

    let label_step = (visible.len() / 5).max(1);
    for (index, row) in visible.iter().enumerate() {
        if index % label_step == 0 || index + 1 == visible.len() {
            let label = if indicator.quarterly {
                let quarter = row
                    .date
                    .get(5..7)
                    .and_then(|month| month.parse::<u8>().ok())
                    .map(|month| (month - 1) / 3 + 1)
                    .unwrap_or_default();
                format!("T{quarter}/{}", row.date.get(2..4).unwrap_or(""))
            } else {
                format!(
                    "{}/{}",
                    row.date.get(5..7).unwrap_or(""),
                    row.date.get(2..4).unwrap_or("")
                )
            };
            svg.push_str(&format!(
                r##"<text x="{:.1}" y="286" text-anchor="middle" fill="#91a196" font-size="10">{label}</text>"##,
                x(index)
            ));
        }
    }
    svg.push_str(r##"<line x1="65" y1="258" x2="875" y2="258" stroke="#435248"/>"##);
    let mut legend = vec![(indicator.primary_label, "#c8a9ff")];
    if let Some(label) = indicator.secondary_label {
        legend.push((label, "#56cfe1"));
    }
    for (index, (label, color)) in legend.into_iter().enumerate() {
        let x_position = 70 + index * 190;
        svg.push_str(&format!(
            r##"<circle cx="{x_position}" cy="305" r="4" fill="{color}"/><text x="{}" y="309" fill="#bcc8bf" font-size="11">{label}</text>"##,
            x_position + 10
        ));
    }
    svg.push_str("</svg>");
    svg
}

fn month_index(date: &str) -> i32 {
    let mut parts = date.split('-');
    let year = parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .unwrap_or_default();
    let month = parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .unwrap_or(1);
    year * 12 + month - 1
}

fn economic_period_label(indicator: EconomicIndicator, date: &str) -> String {
    if indicator.quarterly {
        let year = date.get(0..4).unwrap_or("");
        let quarter = date
            .get(5..7)
            .and_then(|month| month.parse::<u8>().ok())
            .map(|month| (month - 1) / 3 + 1);
        if let Some(quarter) = quarter {
            return format!("T{quarter} {year}");
        }
    }
    format_date(date)
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

fn preview_text(source: &str) -> String {
    let text = Parser::new(source)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.to_string()),
            Event::SoftBreak | Event::HardBreak => Some(" ".to_string()),
            _ => None,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let preview = text.chars().take(150).collect::<String>();
    if text.chars().count() > 150 {
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

async fn add_economic_observation(
    currency_code: &str,
    series_id: &str,
    draft: EconomicObservationDraft,
) -> Result<EconomicObservation, String> {
    let response = Request::post(&format!(
        "{API_BASE}/api/economic/{currency_code}/observations/{series_id}"
    ))
    .json(&draft)
    .map_err(|error| error.to_string())?
    .send()
    .await
    .map_err(|error| error.to_string())?;

    if !response.ok() {
        let status = response.status();
        return Err(response
            .json::<ApiErrorResponse>()
            .await
            .map(|payload| payload.error)
            .unwrap_or_else(|error| format!("API returned HTTP {status}: {error}")));
    }

    response
        .json::<EconomicObservation>()
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

#[cfg(test)]
mod economic_dashboard_tests {
    use super::*;

    fn indicator<'a>(indicators: &'a [EconomicIndicator], id: &str) -> &'a EconomicIndicator {
        indicators
            .iter()
            .find(|indicator| indicator.id == id)
            .unwrap_or_else(|| panic!("missing indicator {id}"))
    }

    #[test]
    fn country_specific_inflation_measures_and_pmi_sources_are_preserved() {
        let cad = economic_indicators("CAD");
        assert!(cad.iter().any(|item| item.id == "cpi_common"));
        assert!(cad.iter().any(|item| item.id == "cpi_trimmed_yoy"));
        assert!(cad.iter().any(|item| item.id == "cpi_median_yoy"));

        let aud = economic_indicators("AUD");
        assert!(indicator(&aud, "cpi_trimmed_yoy").quarterly);
        assert!(indicator(&aud, "cpi_weighted_median_yoy").quarterly);
        assert_eq!(
            indicator(&aud, "cpi_trimmed_yoy").source_name,
            "Reserve Bank of Australia"
        );

        let eur = economic_indicators("EUR");
        assert!(eur.iter().any(|item| item.id == "hicp_headline_yoy"));
        assert!(eur.iter().any(|item| item.id == "hicp_core_yoy"));
        assert!(!eur.iter().any(|item| item.id.starts_with("cpi_")));

        let cny = economic_indicators("CNY");
        assert!(cny
            .iter()
            .any(|item| item.id == "ratingdog_manufacturing_pmi"));
        assert!(!cny.iter().any(|item| item.id.starts_with("sp_global_")));

        for currency in ["EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "CNY"] {
            assert!(!economic_indicators(currency).is_empty(), "{currency}");
            assert!(!economic_indicators(currency)
                .iter()
                .any(|item| item.id.contains("pce")));
        }
        for currency in ["CHF", "NZD"] {
            assert!(!economic_indicators(currency)
                .iter()
                .any(|item| item.id.starts_with("sp_global_")));
        }
    }

    #[test]
    fn quarterly_observations_use_quarter_labels() {
        let eur = economic_indicators("EUR");
        let gdp = indicator(&eur, "gdp_qoq");
        assert_eq!(economic_period_label(*gdp, "2026-07-01"), "T3 2026");

        let aud = economic_indicators("AUD");
        let cpi = indicator(&aud, "cpi_trimmed_yoy");
        assert_eq!(economic_period_label(*cpi, "2026-07-01"), "T3 2026");
    }

    #[test]
    fn line_charts_break_at_missing_publication_periods() {
        let pmi = economic_indicator("EUR", "sp_global_services_pmi").unwrap();
        let gap = [
            EconomicObservation {
                series_id: pmi.id.to_string(),
                date: "2026-08-01".to_string(),
                value: 51.0,
                value_secondary: None,
            },
            EconomicObservation {
                series_id: pmi.id.to_string(),
                date: "2026-10-01".to_string(),
                value: 52.0,
                value_secondary: None,
            },
        ];
        let gap_chart = render_economic_chart(&gap, 3, "line", pmi);
        assert!(!gap_chart.contains(" L "));

        let adjacent = [
            gap[0].clone(),
            EconomicObservation {
                date: "2026-09-01".to_string(),
                value: 51.5,
                ..gap[1].clone()
            },
        ];
        let adjacent_chart = render_economic_chart(&adjacent, 3, "line", pmi);
        assert!(adjacent_chart.contains(" L "));
    }
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
}
