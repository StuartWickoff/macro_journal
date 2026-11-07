//! Binaire du serveur Macro Journal : bootstrap uniquement.
//!
//! La logique de stockage vit dans la bibliothèque `macro_journal_server`
//! (modules `config`, `storage`, `backup`), testable unitairement ; ce
//! fichier se limite à l'assemblage : logging, persistance, routes, écoute.

use std::collections::HashMap;
use std::{env, path::PathBuf, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use macro_journal_server::{
    backup::create_backup,
    config::{workspace_root, StorageLayout},
    storage::{open_or_initialize, StorageError},
};
use macro_journal_shared::{Entry, EntryDraft, HealthResponse};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info};

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    /// Répertoire des snapshots, résolu au démarrage à côté de la base :
    /// jamais dérivé du répertoire courant au moment d'une requête.
    backups_dir: PathBuf,
    economic_data_root: PathBuf,
    /// Sérialise les read-modify-write des historiques CSV économiques :
    /// deux POST concomitants sur la même devise liraient sinon la même
    /// version du fichier et perdraient une mise à jour.
    economic_io_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, FromRow)]
struct DbEntry {
    id: i64,
    currency_code: String,
    title: String,
    content: String,
    created_at: String,
    updated_at: String,
}

impl From<DbEntry> for Entry {
    fn from(row: DbEntry) -> Self {
        Self {
            id: row.id,
            currency_code: row.currency_code,
            title: row.title,
            content: row.content,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

#[derive(Debug, Serialize)]
struct BackupResponse {
    path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EconomicObservation {
    series_id: String,
    date: String,
    value: f64,
    value_secondary: Option<f64>,
}

/// Une ligne du fichier de verdicts du pipeline de recherche : la
/// fiabilité de l'historique d'une série, par bloc de dates, avec la
/// preuve en commentaire. Lecture seule — le fichier reste la source éditable.
#[derive(Clone, Debug, Serialize)]
struct DataVerdict {
    currency: String,
    series_id: String,
    block1: String,
    block2: String,
    verdict: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
struct EconomicObservationDraft {
    date: String,
    value: f64,
    value_secondary: Option<f64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            env::var("RUST_LOG")
                .unwrap_or_else(|_| "macro_journal_server=info,tower_http=info".into()),
        )
        .with_target(false)
        .init();

    // Résolution des chemins : MACRO_JOURNAL_DB > XDG_DATA_HOME > $HOME/.local/share.
    // Toujours absolu par défaut : indépendant du répertoire courant.
    let layout = StorageLayout::resolve()?;

    // Ouverture ou première installation sécurisée (sentinelle `.initialized`,
    // contrôle d'intégrité, détection d'une éventuelle ancienne base relative).
    let pool = match open_or_initialize(&layout, &sqlx::migrate!("./migrations")).await {
        Ok(pool) => pool,
        Err(storage_error) => {
            report_startup_storage_failure(&storage_error);
            return Err(storage_error.into());
        }
    };
    info!("SQLite database: {}", layout.db_path.display());

    let state = AppState {
        pool,
        backups_dir: layout.backups_dir.clone(),
        economic_data_root: workspace_root(),
        economic_io_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    // Clone bon marché (pool Arc) : le pool reste accessible pour la fermeture
    // ordonnée après l'arrêt du serveur.
    let shutdown_pool = state.pool.clone();
    let cors = CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("http://127.0.0.1:8080"),
            HeaderValue::from_static("http://localhost:8080"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE]);

    // En développement, Trunk sert l'UI sur :8080. Après `trunk build --release`,
    // le même serveur peut servir le frontend compilé depuis apps/web/dist,
    // résolu depuis la racine du workspace (indépendant du répertoire courant).
    let dist_dir = workspace_root().join("apps/web/dist");
    let static_files =
        ServeDir::new(&dist_dir).not_found_service(ServeFile::new(dist_dir.join("index.html")));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/entries", get(list_entries).post(create_entry))
        .route(
            "/api/entries/{id}",
            get(get_entry).put(update_entry).delete(delete_entry),
        )
        .route(
            "/api/currencies/{currency_code}/entries",
            get(list_currency_entries).post(create_currency_entry),
        )
        .route(
            "/api/currencies/{currency_code}/entries/{id}",
            axum::routing::put(update_currency_entry),
        )
        .route("/api/backups", post(create_backup_handler))
        .route(
            "/api/economic/{currency_code}/observations",
            get(list_economic_observations),
        )
        .route("/api/verdicts", get(list_verdicts))
        .route(
            "/api/economic/{currency_code}/observations/{series_id}",
            post(add_economic_observation),
        )
        .fallback_service(static_files)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let address = "127.0.0.1:3000";
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!("Macro Journal API listening on http://{address}");
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    server.await?;

    // Fermeture ordonnée du pool : checkpoint WAL avant l'arrêt du processus.
    shutdown_pool.close().await;
    Ok(())
}

/// Journalise lisiblement les échecs de stockage au démarrage : ce sont des
/// situations opérationnelles (base perdue, ancienne base détectée…) qui
/// méritent un message explicite, pas une trace brute.
fn report_startup_storage_failure(error: &StorageError) {
    error!("Macro Journal cannot start: {error}");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn list_economic_observations(
    Path(currency_code): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<EconomicObservation>>, ApiFailure> {
    let path = economic_history_path(&state.economic_data_root, &currency_code)?;
    read_economic_history(&path, &currency_code).await.map(Json)
}

/// Lecture seule des verdicts de fiabilité des données : le pipeline de
/// recherche de l'utilisateur écrit le fichier, l'API se contente de
/// l'exposer. Les corrections restent dans le fichier, jamais ici.
async fn list_verdicts(
    State(state): State<AppState>,
) -> Result<Json<Vec<DataVerdict>>, ApiFailure> {
    let path = verdicts_path(&state.economic_data_root);
    read_verdicts(&path).await.map(Json)
}

fn verdicts_path(root: &std::path::Path) -> PathBuf {
    root.join("economic_data")
        .join("recherches_gemini")
        .join("verdicts.csv")
}

async fn read_verdicts(path: &std::path::Path) -> Result<Vec<DataVerdict>, ApiFailure> {
    let contents = tokio::fs::read_to_string(path).await.map_err(|error| {
        error!(?error, path = %path.display(), "Could not read verdicts file");
        ApiFailure::internal("Verdicts file could not be read.")
    })?;

    let mut lines = contents.lines();
    match lines.next() {
        Some(header) => {
            let columns = split_verdicts_row(header);
            if columns.len() != 6
                || columns.first().map(String::as_str) != Some("currency")
                || columns.get(1).map(String::as_str) != Some("series_id")
            {
                error!(path = %path.display(), "Verdicts file has an invalid CSV header");
                return Err(ApiFailure::internal(
                    "Verdicts file has an invalid CSV header.",
                ));
            }
        }
        None => {
            error!(path = %path.display(), "Verdicts file is empty");
            return Err(ApiFailure::internal("Verdicts file is empty."));
        }
    }

    let mut verdicts = Vec::new();
    for (index, line) in lines.enumerate() {
        let columns = split_verdicts_row(line);
        if columns.len() != 6 {
            error!(line = index + 2, "Verdicts file has an invalid CSV row");
            return Err(ApiFailure::internal(
                "Verdicts file contains an invalid CSV row.",
            ));
        }
        verdicts.push(DataVerdict {
            currency: columns[0].clone(),
            series_id: columns[1].clone(),
            block1: columns[2].clone(),
            block2: columns[3].clone(),
            verdict: columns[4].clone(),
            evidence: columns[5].clone(),
        });
    }
    Ok(verdicts)
}

/// Décodage CSV d'une ligne de verdicts : les preuves sont entre guillemets
/// et contiennent des virgules, un split naïf les casserait. Un guillemet
/// doublé à l'intérieur d'un champ cité vaut un guillemet littéral.
fn split_verdicts_row(line: &str) -> Vec<String> {
    let mut columns = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(char) = chars.next() {
        if in_quotes {
            if char == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(char);
            }
        } else if char == '"' {
            in_quotes = true;
        } else if char == ',' {
            columns.push(std::mem::take(&mut current));
        } else {
            current.push(char);
        }
    }
    columns.push(current);
    columns
}

async fn add_economic_observation(
    State(state): State<AppState>,
    Path((currency_code, series_id)): Path<(String, String)>,
    Json(draft): Json<EconomicObservationDraft>,
) -> Result<(StatusCode, Json<EconomicObservation>), ApiFailure> {
    let path = economic_history_path(&state.economic_data_root, &currency_code)?;
    validate_economic_observation(&currency_code, &series_id, &draft)?;
    // Section critique CSV : verrouillé (read-modify-write) pour éviter
    // la perte de mise à jour entre deux POST économiques concurrents.
    let _io_guard = state.economic_io_lock.lock().await;
    let mut observations = read_economic_history(&path, &currency_code).await?;
    let observation = EconomicObservation {
        series_id,
        date: draft.date,
        value: draft.value,
        value_secondary: draft.value_secondary,
    };
    let latest_month = observations
        .iter()
        .filter(|row| row.series_id == observation.series_id)
        .map(|row| economic_month_index(&row.date))
        .max();
    if latest_month.is_some_and(|latest| economic_month_index(&observation.date) < latest - 36) {
        return Err(ApiFailure::bad_request(
            "Only observations within the latest three years can be saved.",
        ));
    }

    if let Some(existing) = observations.iter_mut().find(|existing| {
        existing.series_id == observation.series_id && existing.date == observation.date
    }) {
        *existing = observation.clone();
    } else {
        observations.push(observation.clone());
    }
    retain_latest_three_years(&mut observations);
    observations.sort_by(|left, right| {
        left.series_id
            .cmp(&right.series_id)
            .then_with(|| left.date.cmp(&right.date))
    });

    let mut csv = String::from("series_id,date,value,value_secondary\n");
    for row in &observations {
        csv.push_str(&format!(
            "{},{},{},{}\n",
            row.series_id,
            row.date,
            row.value,
            row.value_secondary
                .map(|value| value.to_string())
                .unwrap_or_default()
        ));
    }
    let temporary_path = path.with_extension("csv.tmp");
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            error!(?error, currency = %currency_code, "Could not create the economic-data directory");
            ApiFailure::internal("Economic data could not be saved.")
        })?;
    }
    tokio::fs::write(&temporary_path, csv)
        .await
        .map_err(|error| {
            error!(?error, currency = %currency_code, "Could not write temporary economic history");
            ApiFailure::internal("Economic data could not be saved.")
        })?;
    tokio::fs::rename(&temporary_path, &path)
        .await
        .map_err(|error| {
            error!(?error, currency = %currency_code, "Could not publish economic history");
            ApiFailure::internal("Economic data could not be saved.")
        })?;

    Ok((StatusCode::CREATED, Json(observation)))
}

fn retain_latest_three_years(observations: &mut Vec<EconomicObservation>) {
    let mut latest_by_series = HashMap::new();
    for row in observations.iter() {
        let month = economic_month_index(&row.date);
        latest_by_series
            .entry(row.series_id.clone())
            .and_modify(|latest: &mut i32| *latest = (*latest).max(month))
            .or_insert(month);
    }
    observations.retain(|row| {
        latest_by_series
            .get(&row.series_id)
            .is_some_and(|latest| economic_month_index(&row.date) >= latest - 36)
    });
}

fn economic_month_index(date: &str) -> i32 {
    let mut parts = date.split('-');
    let year = parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .unwrap_or_default();
    let month = parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .unwrap_or_default();
    year * 12 + month - 1
}

fn economic_history_path(
    root: &std::path::Path,
    currency_code: &str,
) -> Result<PathBuf, ApiFailure> {
    match currency_code {
        "USD" => Ok(root.join("economic_data/usd/history.csv")),
        "EUR" | "GBP" | "JPY" | "CHF" | "CAD" | "AUD" | "NZD" | "CNY" => Ok(root
            .join("data_eco")
            .join(currency_code.to_ascii_lowercase())
            .join("history.csv")),
        _ => Err(ApiFailure::bad_request("Unknown economic-data currency.")),
    }
}

async fn read_economic_history(
    path: &std::path::Path,
    currency_code: &str,
) -> Result<Vec<EconomicObservation>, ApiFailure> {
    let contents = tokio::fs::read_to_string(path).await.map_err(|error| {
        error!(?error, path = %path.display(), currency = %currency_code, "Could not read economic history");
        ApiFailure::internal("Economic history could not be read.")
    })?;
    let mut lines = contents.lines();
    if lines.next() != Some("series_id,date,value,value_secondary") {
        error!(path = %path.display(), currency = %currency_code, "Economic history has an invalid CSV header");
        return Err(ApiFailure::internal(
            "Economic history has an invalid CSV header.",
        ));
    }

    lines
        .enumerate()
        .map(|(index, line)| {
            let columns = line.split(',').collect::<Vec<_>>();
            if columns.len() != 4 {
                error!(line = index + 2, "Economic history has an invalid CSV row");
                return Err(ApiFailure::internal(
                    "Economic history contains an invalid CSV row.",
                ));
            }
            let value = columns[2].parse::<f64>().map_err(|_| {
                error!(
                    line = index + 2,
                    "Economic history contains an invalid value"
                );
                ApiFailure::internal("Economic history contains an invalid value.")
            })?;
            let value_secondary = if columns[3].is_empty() {
                None
            } else {
                Some(columns[3].parse::<f64>().map_err(|_| {
                    error!(
                        line = index + 2,
                        "Economic history contains an invalid secondary value"
                    );
                    ApiFailure::internal("Economic history contains an invalid value.")
                })?)
            };
            let row = EconomicObservation {
                series_id: columns[0].to_string(),
                date: columns[1].to_string(),
                value,
                value_secondary,
            };
            validate_economic_observation(
                currency_code,
                &row.series_id,
                &EconomicObservationDraft {
                    date: row.date.clone(),
                    value: row.value,
                    value_secondary: row.value_secondary,
                },
            )
            .map_err(|_| {
                error!(
                    line = index + 2,
                    "Economic history contains an invalid observation"
                );
                ApiFailure::internal("Economic history contains an invalid observation.")
            })?;
            Ok(row)
        })
        .collect()
}

fn validate_economic_observation(
    currency_code: &str,
    series_id: &str,
    draft: &EconomicObservationDraft,
) -> Result<(), ApiFailure> {
    let (series, quarterly) = match currency_code {
        "USD" => (
            &[
                "cpi_inflation",
                "fed_target_range",
                "pce_inflation",
                "ppi_inflation",
                "unemployment_rate",
                "nonfarm_payrolls",
                "real_gdp_growth",
                "ism_manufacturing_pmi",
                "ism_services_pmi",
            ][..],
            series_id == "real_gdp_growth",
        ),
        "EUR" => (
            &[
                "policy_rate",
                "hicp_headline_yoy",
                "hicp_core_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
                "sp_global_manufacturing_pmi",
                "sp_global_services_pmi",
            ][..],
            matches!(series_id, "gdp_qoq" | "gdp_yoy"),
        ),
        "GBP" | "JPY" => (
            &[
                "policy_rate",
                "cpi_headline_yoy",
                "cpi_core_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
                "sp_global_manufacturing_pmi",
                "sp_global_services_pmi",
            ][..],
            matches!(series_id, "gdp_qoq" | "gdp_yoy"),
        ),
        "CHF" => (
            &[
                "policy_rate",
                "cpi_headline_yoy",
                "cpi_core_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
            ][..],
            matches!(series_id, "unemployment_rate" | "gdp_qoq" | "gdp_yoy"),
        ),
        "CAD" => (
            &[
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
            ][..],
            matches!(series_id, "gdp_qoq" | "gdp_yoy"),
        ),
        "AUD" => (
            &[
                "policy_rate",
                "cpi_trimmed_yoy",
                "cpi_weighted_median_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
                "sp_global_manufacturing_pmi",
                "sp_global_services_pmi",
            ][..],
            matches!(
                series_id,
                "cpi_trimmed_yoy" | "cpi_weighted_median_yoy" | "ppi_yoy" | "gdp_qoq" | "gdp_yoy"
            ),
        ),
        "NZD" => (
            &[
                "policy_rate",
                "cpi_headline_yoy",
                "cpi_core_proxy_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
            ][..],
            matches!(
                series_id,
                "cpi_headline_yoy"
                    | "cpi_core_proxy_yoy"
                    | "ppi_yoy"
                    | "unemployment_rate"
                    | "gdp_qoq"
                    | "gdp_yoy"
            ),
        ),
        "CNY" => (
            &[
                "policy_rate",
                "cpi_headline_yoy",
                "cpi_core_yoy",
                "ppi_yoy",
                "unemployment_rate",
                "gdp_qoq",
                "gdp_yoy",
                "ratingdog_manufacturing_pmi",
                "ratingdog_services_pmi",
            ][..],
            matches!(series_id, "gdp_qoq" | "gdp_yoy"),
        ),
        _ => return Err(ApiFailure::bad_request("Unknown economic-data currency.")),
    };
    if !series.contains(&series_id) {
        return Err(ApiFailure::bad_request(
            "Unknown economic indicator for this currency.",
        ));
    }
    if !valid_iso_date(&draft.date) {
        return Err(ApiFailure::bad_request(
            "Date must be a valid YYYY-MM-DD date.",
        ));
    }
    if !draft.value.is_finite()
        || draft
            .value_secondary
            .is_some_and(|value| !value.is_finite())
    {
        return Err(ApiFailure::bad_request("Values must be finite numbers."));
    }
    let has_secondary = currency_code == "USD"
        && matches!(
            series_id,
            "cpi_inflation" | "fed_target_range" | "pce_inflation" | "ppi_inflation"
        );
    if draft.value_secondary.is_some() != has_secondary {
        return Err(ApiFailure::bad_request(
            "This indicator has an invalid number of values.",
        ));
    }
    if currency_code == "USD"
        && series_id == "fed_target_range"
        && draft
            .value_secondary
            .is_some_and(|upper| draft.value > upper)
    {
        return Err(ApiFailure::bad_request(
            "The lower Fed target rate must not exceed the upper rate.",
        ));
    }
    if (series_id.ends_with("_pmi") || series_id.ends_with("_services_pmi"))
        && !(0.0..=100.0).contains(&draft.value)
    {
        return Err(ApiFailure::bad_request(
            "PMI values must be between 0 and 100.",
        ));
    }
    if currency_code == "USD" && series_id == "nonfarm_payrolls" && draft.value.fract() != 0.0 {
        return Err(ApiFailure::bad_request(
            "Nonfarm payroll changes must be whole thousands (K).",
        ));
    }
    if currency_code != "USD" {
        let day = draft.date.get(8..10).unwrap_or_default();
        if day != "01"
            || (quarterly && !matches!(draft.date.get(5..7), Some("01" | "04" | "07" | "10")))
        {
            return Err(ApiFailure::bad_request(
                "Economic observation date does not match its monthly or quarterly period.",
            ));
        }
    }
    Ok(())
}

fn valid_iso_date(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        parts[0].parse::<i32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) else {
        return false;
    };
    if parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

async fn list_entries(State(state): State<AppState>) -> Result<Json<Vec<Entry>>, ApiFailure> {
    let rows = sqlx::query_as::<_, DbEntry>(
        "SELECT id, currency_code, title, content, created_at, updated_at FROM entries ORDER BY updated_at DESC, id DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiFailure::database)?;

    Ok(Json(rows.into_iter().map(Entry::from).collect()))
}

async fn list_currency_entries(
    State(state): State<AppState>,
    Path(currency_code): Path<String>,
) -> Result<Json<Vec<Entry>>, ApiFailure> {
    validate_currency_code(&currency_code, true)?;
    let rows = sqlx::query_as::<_, DbEntry>(
        "SELECT id, currency_code, title, content, created_at, updated_at FROM entries WHERE currency_code = ? ORDER BY updated_at DESC, id DESC",
    )
    .bind(currency_code)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiFailure::database)?;

    Ok(Json(rows.into_iter().map(Entry::from).collect()))
}

async fn get_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Entry>, ApiFailure> {
    let row = sqlx::query_as::<_, DbEntry>(
        "SELECT id, currency_code, title, content, created_at, updated_at FROM entries WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiFailure::database)?
    .ok_or(ApiFailure::not_found())?;

    Ok(Json(Entry::from(row)))
}

async fn create_entry(
    State(state): State<AppState>,
    Json(draft): Json<EntryDraft>,
) -> Result<impl IntoResponse, ApiFailure> {
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(ApiFailure::bad_request("The title cannot be empty."));
    }

    let row = sqlx::query_as::<_, DbEntry>(
        "INSERT INTO entries (title, content) VALUES (?, ?) RETURNING id, currency_code, title, content, created_at, updated_at",
    )
    .bind(title)
    .bind(draft.content)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiFailure::database)?;

    Ok((StatusCode::CREATED, Json(Entry::from(row))))
}

async fn create_currency_entry(
    State(state): State<AppState>,
    Path(currency_code): Path<String>,
    Json(draft): Json<EntryDraft>,
) -> Result<impl IntoResponse, ApiFailure> {
    validate_currency_code(&currency_code, false)?;
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(ApiFailure::bad_request("The title cannot be empty."));
    }

    let row = sqlx::query_as::<_, DbEntry>(
        "INSERT INTO entries (currency_code, title, content) VALUES (?, ?, ?) RETURNING id, currency_code, title, content, created_at, updated_at",
    )
    .bind(currency_code)
    .bind(title)
    .bind(draft.content)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiFailure::database)?;

    Ok((StatusCode::CREATED, Json(Entry::from(row))))
}

async fn update_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(draft): Json<EntryDraft>,
) -> Result<Json<Entry>, ApiFailure> {
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(ApiFailure::bad_request("The title cannot be empty."));
    }

    let row = sqlx::query_as::<_, DbEntry>(
        "UPDATE entries SET title = ?, content = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? RETURNING id, currency_code, title, content, created_at, updated_at",
    )
    .bind(title)
    .bind(draft.content)
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiFailure::database)?
    .ok_or(ApiFailure::not_found())?;

    Ok(Json(Entry::from(row)))
}

async fn update_currency_entry(
    State(state): State<AppState>,
    Path((currency_code, id)): Path<(String, i64)>,
    Json(draft): Json<EntryDraft>,
) -> Result<Json<Entry>, ApiFailure> {
    validate_currency_code(&currency_code, true)?;
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(ApiFailure::bad_request("The title cannot be empty."));
    }

    let row = sqlx::query_as::<_, DbEntry>(
        "UPDATE entries SET title = ?, content = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND currency_code = ? RETURNING id, currency_code, title, content, created_at, updated_at",
    )
    .bind(title)
    .bind(draft.content)
    .bind(id)
    .bind(currency_code)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiFailure::database)?
    .ok_or(ApiFailure::not_found())?;

    Ok(Json(Entry::from(row)))
}

fn validate_currency_code(code: &str, include_unassigned: bool) -> Result<(), ApiFailure> {
    let valid = matches!(
        code,
        "USD" | "EUR" | "GBP" | "JPY" | "CHF" | "CAD" | "AUD" | "NZD" | "CNY"
    ) || (include_unassigned && code == "UNASSIGNED");
    if valid {
        Ok(())
    } else {
        Err(ApiFailure::bad_request("Unknown currency code."))
    }
}

async fn delete_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiFailure> {
    let result = sqlx::query("DELETE FROM entries WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(ApiFailure::database)?;

    if result.rows_affected() == 0 {
        return Err(ApiFailure::not_found());
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Crée un snapshot SQLite vérifié (`VACUUM INTO` + `integrity_check` +
/// comparaison tables/compteurs) dans le répertoire des sauvegardes
/// mémorisé au démarrage.
async fn create_backup_handler(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<BackupResponse>), ApiFailure> {
    match create_backup(&state.pool, &state.backups_dir).await {
        Ok(path) => Ok((
            StatusCode::CREATED,
            Json(BackupResponse {
                path: path.display().to_string(),
            }),
        )),
        Err(backup_error) => {
            error!("Backup failed: {backup_error}");
            Err(ApiFailure::internal(
                "The database backup could not be created and verified.",
            ))
        }
    }
}

#[derive(Debug)]
struct ApiFailure {
    status: StatusCode,
    message: String,
}

impl ApiFailure {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "Entry not found in this currency folder.".into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn database(error: sqlx::Error) -> Self {
        error!(?error, "Database request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Database request failed.".into(),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ApiError {
                error: self.message,
            }),
        )
            .into_response()
    }
}

/// Tests intégration du routeur complet sur une base SQLite isolée en
/// répertoire temporaire (jamais la base personnelle).
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use macro_journal_server::config::StorageLayout;
    use tower::ServiceExt;

    #[test]
    fn split_verdicts_row_keeps_quoted_commas() {
        let columns = split_verdicts_row(
            "EUR,policy_rate,OK,WRONG,WARN,\"ECB: 3.00 (file 2.75), corrigé après\"",
        );
        assert_eq!(columns.len(), 6);
        assert_eq!(columns[0], "EUR");
        assert_eq!(columns[5], "ECB: 3.00 (file 2.75), corrigé après");

        let plain = split_verdicts_row("USD,cpi_headline_yoy,OK,OK,OK,rien à signaler");
        assert_eq!(plain.len(), 6);
        assert_eq!(plain[4], "OK");
    }

    #[tokio::test]
    async fn read_verdicts_parses_quoted_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("verdicts.csv");
        std::fs::write(
            &path,
            "currency,series_id,block1,block2,verdict,evidence\n\
             EUR,policy_rate,OK,WRONG,WARN,\"ECB: Jan-2025 3.00 (file 2.75), fixed later\"\n",
        )
        .unwrap();
        let verdicts = read_verdicts(&path).await.unwrap();
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].currency, "EUR");
        assert_eq!(verdicts[0].series_id, "policy_rate");
        assert_eq!(verdicts[0].block2, "WRONG");
        assert_eq!(
            verdicts[0].evidence,
            "ECB: Jan-2025 3.00 (file 2.75), fixed later"
        );
    }

    #[tokio::test]
    async fn read_verdicts_rejects_invalid_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("verdicts.csv");
        std::fs::write(&path, "colone,autre\nvaleur,valeur\n").unwrap();
        assert!(read_verdicts(&path).await.is_err());
    }

    async fn test_app(dir: &std::path::Path) -> Router {
        let layout =
            StorageLayout::with_legacy_candidates(dir.join("macro_journal.db"), Vec::new());
        let pool = open_or_initialize(&layout, &sqlx::migrate!("./migrations"))
            .await
            .unwrap();
        for currency_code in [
            "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "CNY",
        ] {
            let path = economic_history_path(dir, currency_code).unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "series_id,date,value,value_secondary\n").unwrap();
        }
        Router::new()
            .route("/api/health", get(health))
            .route("/api/entries", get(list_entries).post(create_entry))
            .route(
                "/api/entries/{id}",
                get(get_entry).put(update_entry).delete(delete_entry),
            )
            .route(
                "/api/currencies/{currency_code}/entries",
                get(list_currency_entries).post(create_currency_entry),
            )
            .route(
                "/api/currencies/{currency_code}/entries/{id}",
                axum::routing::put(update_currency_entry),
            )
            .route("/api/backups", post(create_backup_handler))
            .route(
                "/api/economic/{currency_code}/observations",
                get(list_economic_observations),
            )
            .route(
                "/api/economic/{currency_code}/observations/{series_id}",
                post(add_economic_observation),
            )
            .with_state(AppState {
                pool,
                backups_dir: layout.backups_dir,
                economic_data_root: dir.to_path_buf(),
                economic_io_lock: Arc::new(tokio::sync::Mutex::new(())),
            })
    }

    fn health_request() -> Request<Body> {
        Request::get("/api/health").body(Body::empty()).unwrap()
    }

    fn create_entry_request(title: &str, content: &str) -> Request<Body> {
        let body = serde_json::json!({ "title": title, "content": content });
        Request::post("/api/entries")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn create_currency_entry_request(
        currency_code: &str,
        title: &str,
        content: &str,
    ) -> Request<Body> {
        let body = serde_json::json!({ "title": title, "content": content });
        Request::post(format!("/api/currencies/{currency_code}/entries"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn add_economic_observation_request(
        series_id: &str,
        date: &str,
        value: f64,
        value_secondary: Option<f64>,
    ) -> Request<Body> {
        add_currency_economic_observation_request("USD", series_id, date, value, value_secondary)
    }

    fn add_currency_economic_observation_request(
        currency_code: &str,
        series_id: &str,
        date: &str,
        value: f64,
        value_secondary: Option<f64>,
    ) -> Request<Body> {
        let body = serde_json::json!({
            "date": date,
            "value": value,
            "value_secondary": value_secondary,
        });
        Request::post(format!(
            "/api/economic/{currency_code}/observations/{series_id}"
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
    }

    async fn body_string(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn health_route_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;
        let response = app.oneshot(health_request()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response).await;
        assert!(body.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn entries_crud_still_works() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        let response = app
            .clone()
            .oneshot(create_entry_request("Titre", "Contenu"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(Request::get("/api/entries").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response).await;
        assert!(body.contains("Titre"));

        let response = app
            .clone()
            .oneshot(
                Request::put("/api/entries/1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Titre 2", "content": "Contenu 2" })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::delete("/api/entries/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app
            .oneshot(Request::get("/api/entries/1").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn currency_entries_are_isolated_by_folder() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        let response = app
            .clone()
            .oneshot(create_entry_request("Ancienne note", "à classer"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(create_currency_entry_request(
                "USD",
                "Vue dollar",
                "USD only",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let usd_entry: serde_json::Value =
            serde_json::from_str(&body_string(response).await).unwrap();
        assert_eq!(usd_entry["currency_code"], "USD");
        let usd_id = usd_entry["id"].as_i64().unwrap();

        let response = app
            .clone()
            .oneshot(create_currency_entry_request("JPY", "Vue yen", "JPY only"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/currencies/USD/entries")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let usd_entries: serde_json::Value =
            serde_json::from_str(&body_string(response).await).unwrap();
        assert_eq!(usd_entries.as_array().unwrap().len(), 1);
        assert_eq!(usd_entries[0]["title"], "Vue dollar");

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/currencies/UNASSIGNED/entries")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let unassigned: serde_json::Value =
            serde_json::from_str(&body_string(response).await).unwrap();
        assert_eq!(unassigned.as_array().unwrap().len(), 1);
        assert_eq!(unassigned[0]["title"], "Ancienne note");

        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/api/currencies/JPY/entries/{usd_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "title": "Mauvais dossier", "content": "x" })
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = app
            .oneshot(create_currency_entry_request("INVALID", "Titre", "Contenu"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn economic_observations_are_local_and_same_period_updates_existing_row() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        let first = app
            .clone()
            .oneshot(add_economic_observation_request(
                "unemployment_rate",
                "2026-10-01",
                4.2,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);

        let revision = app
            .clone()
            .oneshot(add_economic_observation_request(
                "unemployment_rate",
                "2026-10-01",
                4.1,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(revision.status(), StatusCode::CREATED);

        let response = app
            .oneshot(
                Request::get("/api/economic/USD/observations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let rows: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["value"], 4.1);
        assert!(
            std::fs::read_to_string(economic_history_path(dir.path(), "USD").unwrap())
                .unwrap()
                .contains("unemployment_rate,2026-10-01,4.1,")
        );
    }

    #[tokio::test]
    async fn concurrent_economic_posts_do_not_lose_observations() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        // Scénario 1 : des POST simultanés sur des mois distincts coexistent tous.
        let mut handles = Vec::new();
        for month in 1..=6 {
            let app = app.clone();
            let date = format!("2026-{:02}-01", month);
            handles.push(tokio::spawn(async move {
                let response = app
                    .clone()
                    .oneshot(add_economic_observation_request(
                        "unemployment_rate",
                        &date,
                        4.0,
                        None,
                    ))
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::CREATED);
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        // Scénario 2 : deux POST simultanés sur la même période produisent
        // exactement une ligne finale gagnante, intégrale (ni perdue, ni
        // entremêlée).
        let (left, right) = tokio::join!(
            async {
                app.clone()
                    .oneshot(add_economic_observation_request(
                        "ism_services_pmi",
                        "2026-10-01",
                        50.0,
                        None,
                    ))
                    .await
            },
            async {
                app.clone()
                    .oneshot(add_economic_observation_request(
                        "ism_services_pmi",
                        "2026-10-01",
                        51.0,
                        None,
                    ))
                    .await
            },
        );
        assert_eq!(left.unwrap().status(), StatusCode::CREATED);
        assert_eq!(right.unwrap().status(), StatusCode::CREATED);

        let csv =
            std::fs::read_to_string(economic_history_path(dir.path(), "USD").unwrap()).unwrap();
        let unemployment_rows = csv
            .lines()
            .filter(|line| line.starts_with("unemployment_rate,2026-"))
            .count();
        assert_eq!(unemployment_rows, 6);
        let ism_rows = csv
            .lines()
            .filter(|line| line.starts_with("ism_services_pmi,2026-10-01"))
            .count();
        assert_eq!(ism_rows, 1);
        assert!(
            csv.contains("ism_services_pmi,2026-10-01,50,")
                || csv.contains("ism_services_pmi,2026-10-01,51,")
        );
    }

    #[tokio::test]
    async fn cpi_and_ppi_observations_accept_primary_and_secondary_values() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        for series_id in ["cpi_inflation", "ppi_inflation"] {
            let response = app
                .clone()
                .oneshot(add_economic_observation_request(
                    series_id,
                    "2026-10-01",
                    3.7,
                    Some(2.6),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let csv =
            std::fs::read_to_string(economic_history_path(dir.path(), "USD").unwrap()).unwrap();
        assert!(csv.contains("cpi_inflation,2026-10-01,3.7,2.6"));
        assert!(csv.contains("ppi_inflation,2026-10-01,3.7,2.6"));
    }

    #[tokio::test]
    async fn ism_services_pmi_observation_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;
        let response = app
            .oneshot(add_economic_observation_request(
                "ism_services_pmi",
                "2026-10-01",
                54.9,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(
            std::fs::read_to_string(economic_history_path(dir.path(), "USD").unwrap())
                .unwrap()
                .contains("ism_services_pmi,2026-10-01,54.9,")
        );
    }

    #[tokio::test]
    async fn economic_data_routes_keep_currency_series_and_quarterly_periods_separate() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        for (currency, series, date, value) in [
            ("CAD", "cpi_common", "2026-10-01", 2.4),
            ("CAD", "cpi_trimmed_yoy", "2026-10-01", 2.5),
            ("CAD", "cpi_median_yoy", "2026-10-01", 2.6),
            ("AUD", "cpi_trimmed_yoy", "2026-07-01", 3.7),
            ("AUD", "cpi_weighted_median_yoy", "2026-07-01", 3.6),
            ("NZD", "unemployment_rate", "2026-10-01", 5.7),
            ("EUR", "gdp_qoq", "2026-07-01", 0.2),
            ("CNY", "ratingdog_manufacturing_pmi", "2026-10-01", 50.8),
        ] {
            let response = app
                .clone()
                .oneshot(add_currency_economic_observation_request(
                    currency, series, date, value, None,
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::CREATED,
                "{currency} {series}"
            );
        }

        let invalid_quarter = app
            .clone()
            .oneshot(add_currency_economic_observation_request(
                "AUD",
                "cpi_trimmed_yoy",
                "2026-08-01",
                3.7,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(invalid_quarter.status(), StatusCode::BAD_REQUEST);

        let response = app
            .clone()
            .oneshot(
                Request::get("/api/economic/AUD/observations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let rows: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 2);

        let aud_csv =
            std::fs::read_to_string(economic_history_path(dir.path(), "AUD").unwrap()).unwrap();
        let cad_csv =
            std::fs::read_to_string(economic_history_path(dir.path(), "CAD").unwrap()).unwrap();
        assert!(aud_csv.contains("cpi_trimmed_yoy,2026-07-01,3.7,"));
        assert!(!aud_csv.contains("cpi_common"));
        assert!(cad_csv.contains("cpi_common,2026-10-01,2.4,"));
        assert!(!cad_csv.contains("cpi_weighted_median_yoy"));
    }

    #[tokio::test]
    async fn checked_in_economic_histories_match_their_currency_schema() {
        let root = workspace_root();
        for currency_code in ["EUR", "GBP", "JPY", "CHF", "CAD", "AUD", "NZD", "CNY"] {
            let path = economic_history_path(&root, currency_code).unwrap();
            let rows = read_economic_history(&path, currency_code)
                .await
                .unwrap_or_else(|error| panic!("{currency_code} history is invalid: {error:?}"));
            assert!(!rows.is_empty(), "{currency_code} history is empty");
        }
    }

    #[tokio::test]
    async fn economic_observations_reject_invalid_periods_and_unexpected_indicators() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        for request in [
            add_economic_observation_request("unemployment_rate", "2026-02-30", 4.2, None),
            add_economic_observation_request("outside_usd", "2026-10-01", 4.2, None),
            add_economic_observation_request("ism_manufacturing_pmi", "2026-10-01", 101.0, None),
            add_economic_observation_request("ism_services_pmi", "2026-10-01", 101.0, None),
            add_economic_observation_request("nonfarm_payrolls", "2026-10-01", 12.5, None),
            add_economic_observation_request("cpi_inflation", "2026-10-01", 3.7, None),
            add_economic_observation_request("ppi_inflation", "2026-10-01", 3.7, None),
        ] {
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn economic_history_is_capped_at_three_years_per_indicator() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;
        std::fs::write(
            economic_history_path(dir.path(), "USD").unwrap(),
            concat!(
                "series_id,date,value,value_secondary\n",
                "unemployment_rate,2023-10-01,3.8,\n",
                "unemployment_rate,2023-11-01,3.9,\n",
                "unemployment_rate,2026-10-01,4.2,\n",
            ),
        )
        .unwrap();

        let response = app
            .clone()
            .oneshot(add_economic_observation_request(
                "unemployment_rate",
                "2026-11-01",
                4.1,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .oneshot(
                Request::get("/api/economic/USD/observations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let rows: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
        let dates = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["date"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(dates, ["2023-11-01", "2026-10-01", "2026-11-01"]);
    }

    #[tokio::test]
    async fn economic_observation_older_than_retention_window_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;
        std::fs::write(
            economic_history_path(dir.path(), "USD").unwrap(),
            concat!(
                "series_id,date,value,value_secondary\n",
                "unemployment_rate,2026-11-01,4.1,\n",
            ),
        )
        .unwrap();

        let response = app
            .oneshot(add_economic_observation_request(
                "unemployment_rate",
                "2023-10-01",
                3.8,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            std::fs::read_to_string(economic_history_path(dir.path(), "USD").unwrap()).unwrap(),
            concat!(
                "series_id,date,value,value_secondary\n",
                "unemployment_rate,2026-11-01,4.1,\n",
            )
        );
    }

    #[tokio::test]
    async fn backup_route_creates_verified_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path()).await;

        let response = app
            .clone()
            .oneshot(create_entry_request("Avec sauvegarde", "x"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .oneshot(Request::post("/api/backups").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = body_string(response).await;
        assert!(body.contains("macro_journal-"), "réponse : {body}");

        // Le snapshot annoncé existe réellement.
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let path = parsed["path"].as_str().unwrap();
        assert!(std::path::Path::new(path).is_file());
    }
}
