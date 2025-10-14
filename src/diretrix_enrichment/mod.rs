use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{NaiveDate, NaiveDateTime};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};
use uuid::Uuid;

const DEFAULT_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Error)]
pub enum EnrichmentError {
    #[error("Missing configuration: {0}")]
    MissingConfig(&'static str),
    #[error("Diretrix request failed with status {status}: {message}")]
    HttpFailure { status: StatusCode, message: String },
}

#[derive(Clone, Debug)]
pub struct DiretrixClient {
    http: reqwest::Client,
    base_url: String,
    username: String,
    password: String,
}

impl DiretrixClient {
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("DIRETRIX_BASE_URL")
            .map_err(|_| EnrichmentError::MissingConfig("DIRETRIX_BASE_URL"))?;
        let username = std::env::var("DIRETRIX_USER")
            .map_err(|_| EnrichmentError::MissingConfig("DIRETRIX_USER"))?;
        let password = std::env::var("DIRETRIX_PASS")
            .map_err(|_| EnrichmentError::MissingConfig("DIRETRIX_PASS"))?;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .danger_accept_invalid_certs(false)
            .use_rustls_tls()
            .build()
            .context("Unable to construct reqwest client")?;

        Ok(Self {
            http,
            base_url,
            username,
            password,
        })
    }

    fn auth_request(&self, url: String) -> reqwest::RequestBuilder {
        self.http
            .get(url)
            .basic_auth(&self.username, Some(&self.password))
            .header(reqwest::header::ACCEPT, "application/json")
    }

    pub async fn pessoa_por_cpf(&self, cpf: &str) -> Result<Option<DiretrixPerson>> {
        if cpf.trim().is_empty() {
            return Ok(None);
        }

        let url = format!("{}/pessoas/{cpf}", self.base_url.trim_end_matches('/'));
        let resp = self
            .auth_request(url)
            .send()
            .await
            .context("Failed to execute CPF lookup")?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let message = resp.text().await.unwrap_or_default();
            return Err(EnrichmentError::HttpFailure { status, message }.into());
        }

        let person: DiretrixPerson = resp
            .json()
            .await
            .context("Failed to parse CPF lookup payload")?;
        Ok(Some(person))
    }

    pub async fn seed_by(&self, seed: SeedQuery<'_>) -> Result<Option<serde_json::Value>> {
        let (path, key, value) = match seed {
            SeedQuery::Email(value) => ("emails", "email", value),
            SeedQuery::Telefone(value) => ("telefones", "telefone", value),
            SeedQuery::Nome(value) => ("pessoas", "nome", value),
        };

        if value.trim().is_empty() {
            return Ok(None);
        }

        let url = format!(
            "{}/{path}?{key}={}",
            self.base_url.trim_end_matches('/'),
            urlencoding::encode(value.trim())
        );

        let resp = self
            .auth_request(url)
            .send()
            .await
            .context("Failed to execute seed query")?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let message = resp.text().await.unwrap_or_default();
            return Err(EnrichmentError::HttpFailure { status, message }.into());
        }

        let value: serde_json::Value = resp.json().await.context("Failed to parse seed payload")?;

        Ok(Some(value))
    }
}

#[derive(Debug)]
pub enum SeedQuery<'a> {
    Email(&'a str),
    Telefone(&'a str),
    Nome(&'a str),
}

#[derive(Debug, Deserialize)]
pub struct DiretrixPerson {
    #[serde(rename = "cpf")]
    pub cpf: Option<String>,
    #[serde(rename = "nome")]
    pub name: Option<String>,
    #[serde(rename = "dataNascimento")]
    pub birth_date: Option<String>,
    #[serde(rename = "sexo")]
    pub sex: Option<String>,
    #[serde(rename = "nomeMae")]
    pub mother_name: Option<String>,
    #[serde(rename = "nomePai")]
    pub father_name: Option<String>,
    #[serde(rename = "rg")]
    pub rg: Option<String>,
    #[serde(default)]
    pub emails: Vec<DiretrixEmail>,
    #[serde(default)]
    pub telefones: Vec<DiretrixPhone>,
    #[serde(default)]
    pub enderecos: Vec<DiretrixAddress>,
}

#[derive(Debug, Deserialize)]
pub struct DiretrixEmail {
    #[serde(rename = "email")]
    pub email: Option<String>,
    #[serde(rename = "ranking")]
    pub ranking: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct DiretrixPhone {
    #[serde(rename = "ddd")]
    pub ddd: Option<String>,
    #[serde(rename = "numero")]
    pub number: Option<String>,
    #[serde(rename = "operadora")]
    pub operator_: Option<String>,
    #[serde(rename = "tipo")]
    pub kind: Option<String>,
    #[serde(rename = "ranking")]
    pub ranking: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct DiretrixAddress {
    #[serde(rename = "logradouro")]
    pub street: Option<String>,
    #[serde(rename = "numero")]
    pub number: Option<String>,
    #[serde(rename = "bairro")]
    pub neighborhood: Option<String>,
    #[serde(rename = "cidade")]
    pub city: Option<String>,
    #[serde(rename = "uf")]
    pub uf: Option<String>,
    #[serde(rename = "cep")]
    pub postal_code: Option<String>,
    #[serde(rename = "complemento")]
    pub complement: Option<String>,
    #[serde(rename = "ranking")]
    pub ranking: Option<i32>,
    #[serde(rename = "latitude")]
    pub latitude: Option<String>,
    #[serde(rename = "longitude")]
    pub longitude: Option<String>,
    #[serde(rename = "ddd")]
    pub ddd: Option<String>,
    #[serde(rename = "tipoLogradouro")]
    pub street_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetCustomerData {
    pub base: CustomerBase,
    pub emails: Vec<CustomerEmail>,
    pub phones: Vec<CustomerPhone>,
    pub addresses: Vec<CustomerAddress>,
}

// Workbuscas API response structure
#[derive(Debug, Deserialize)]
pub struct WorkbuscasResponse {
    #[serde(rename = "DadosBasicos")]
    pub dados_basicos: Option<WorkbuscasBasicos>,
    #[serde(rename = "emails")]
    pub emails: Option<Vec<WorkbuscasEmail>>,
    #[serde(rename = "telefones")]
    pub telefones: Option<Vec<WorkbuscasPhone>>,
    #[serde(rename = "enderecos")]
    pub enderecos: Option<Vec<WorkbuscasAddress>>,
}

#[derive(Debug, Deserialize)]
pub struct WorkbuscasBasicos {
    pub nome: Option<String>,
    pub cpf: Option<String>,
    #[serde(rename = "dataNascimento")]
    pub data_nascimento: Option<String>,
    pub sexo: Option<String>,
    #[serde(rename = "nomeMae")]
    pub nome_mae: Option<String>,
    #[serde(rename = "nomePai")]
    pub nome_pai: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkbuscasEmail {
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkbuscasPhone {
    pub ddd: Option<String>,
    pub numero: Option<String>,
    pub operadora: Option<String>,
    pub tipo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkbuscasAddress {
    pub logradouro: Option<String>,
    pub numero: Option<String>,
    pub bairro: Option<String>,
    pub cidade: Option<String>,
    pub uf: Option<String>,
    pub cep: Option<String>,
}

impl From<WorkbuscasResponse> for GetCustomerData {
    fn from(wb: WorkbuscasResponse) -> Self {
        let base = if let Some(dados) = wb.dados_basicos {
            CustomerBase {
                id: dados.cpf.clone().unwrap_or_default(),
                name: dados.nome.unwrap_or_default(),
                cpf: dados.cpf,
                birth_date: dados.data_nascimento,
                sex: dados.sexo,
                mother_name: dados.nome_mae,
                father_name: dados.nome_pai,
                rg: None,
            }
        } else {
            CustomerBase {
                id: String::new(),
                name: String::new(),
                cpf: None,
                birth_date: None,
                sex: None,
                mother_name: None,
                father_name: None,
                rg: None,
            }
        };

        let emails = wb.emails.unwrap_or_default()
            .into_iter()
            .filter_map(|e| e.email.map(|email| CustomerEmail {
                email,
                ranking: None,
            }))
            .collect();

        let phones = wb.telefones.unwrap_or_default()
            .into_iter()
            .map(|p| CustomerPhone {
                ddd: p.ddd,
                number: p.numero,
                operator_: p.operadora,
                kind: p.tipo,
                ranking: None,
            })
            .collect();

        let addresses = wb.enderecos.unwrap_or_default()
            .into_iter()
            .map(|a| CustomerAddress {
                street: a.logradouro,
                number: a.numero,
                neighborhood: a.bairro,
                city: a.cidade,
                uf: a.uf,
                postal_code: a.cep,
                complement: None,
                ranking: None,
                latitude: None,
                longitude: None,
                ddd: None,
                street_type: None,
            })
            .collect();

        GetCustomerData {
            base,
            emails,
            phones,
            addresses,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerBase {
    pub id: String,
    pub name: String,
    pub cpf: Option<String>,
    pub birth_date: Option<String>,
    pub sex: Option<String>,
    pub mother_name: Option<String>,
    pub father_name: Option<String>,
    pub rg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerEmail {
    pub email: String,
    pub ranking: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerPhone {
    pub ddd: Option<String>,
    pub number: Option<String>,
    pub operator_: Option<String>,
    pub kind: Option<String>,
    pub ranking: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerAddress {
    pub street: Option<String>,
    pub number: Option<String>,
    pub neighborhood: Option<String>,
    pub city: Option<String>,
    pub uf: Option<String>,
    pub postal_code: Option<String>,
    pub complement: Option<String>,
    pub ranking: Option<i32>,
    pub latitude: Option<String>,
    pub longitude: Option<String>,
    pub ddd: Option<String>,
    pub street_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentRequest {
    pub cpf: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

pub async fn enrich_person(
    client: &DiretrixClient,
    request: EnrichmentRequest,
) -> Result<Option<GetCustomerData>> {
    if let Some(cpf) = request
        .cpf
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(person) = client.pessoa_por_cpf(cpf).await? {
            return Ok(Some(map_person(person)));
        }
    }

    let mut candidate: Option<(Option<String>, f64)> = None;

    if let Some(email) = request
        .email
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(seed_value) = client.seed_by(SeedQuery::Email(email)).await? {
            if let Some((cpf, score)) = extract_best_candidate(seed_value, request.name.as_deref())
            {
                if candidate.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                    candidate = Some((cpf, score));
                }
            }
        }
    }

    if candidate
        .as_ref()
        .and_then(|(cpf, _)| cpf.clone())
        .is_none()
    {
        if let Some(phone) = request
            .phone
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(seed_value) = client.seed_by(SeedQuery::Telefone(phone)).await? {
                if let Some((cpf, score)) =
                    extract_best_candidate(seed_value, request.name.as_deref())
                {
                    if candidate.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                        candidate = Some((cpf, score));
                    }
                }
            }
        }
    }

    if candidate
        .as_ref()
        .and_then(|(cpf, _)| cpf.clone())
        .is_none()
    {
        if let Some(name) = request
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(seed_value) = client.seed_by(SeedQuery::Nome(name)).await? {
                if let Some((cpf, score)) = extract_best_candidate(seed_value, Some(name)) {
                    if candidate.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                        candidate = Some((cpf, score));
                    }
                }
            }
        }
    }

    let cpf = match candidate.and_then(|(cpf, _)| cpf) {
        Some(cpf) => cpf,
        None => return Ok(None),
    };

    if let Some(person) = client.pessoa_por_cpf(&cpf).await? {
        return Ok(Some(map_person(person)));
    }

    Ok(None)
}

fn extract_best_candidate(
    value: serde_json::Value,
    reference_name: Option<&str>,
) -> Option<(Option<String>, f64)> {
    match value {
        serde_json::Value::Array(items) => {
            let mut best: Option<(Option<String>, f64)> = None;
            for item in items {
                let candidate_cpf = item
                    .get("cpf")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let candidate_name = item.get("nome").and_then(|v| v.as_str());

                let score =
                    if let (Some(reference), Some(candidate)) = (reference_name, candidate_name) {
                        cosine_similarity(reference, candidate)
                    } else {
                        0.0
                    };

                if reference_name.is_some() && score < 0.5 {
                    continue;
                }

                if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                    best = Some((candidate_cpf.clone(), score));
                }
            }

            best
        }
        serde_json::Value::Object(obj) => {
            let cpf = obj
                .get("cpf")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some((cpf, 1.0))
        }
        _ => None,
    }
}

fn map_person(person: DiretrixPerson) -> GetCustomerData {
    GetCustomerData {
        base: CustomerBase {
            id: Uuid::new_v4().to_string(),
            name: person.name.unwrap_or_default(),
            cpf: person.cpf,
            birth_date: person.birth_date.and_then(|d| date_br_to_iso(&d)),
            sex: person.sex,
            mother_name: person.mother_name,
            father_name: person.father_name,
            rg: person.rg,
        },
        emails: person
            .emails
            .into_iter()
            .filter_map(|e| {
                e.email.map(|mail| CustomerEmail {
                    email: mail,
                    ranking: e.ranking,
                })
            })
            .collect(),
        phones: person
            .telefones
            .into_iter()
            .map(|p| CustomerPhone {
                ddd: p.ddd,
                number: p.number,
                operator_: p.operator_,
                kind: p.kind,
                ranking: p.ranking,
            })
            .collect(),
        addresses: person
            .enderecos
            .into_iter()
            .map(|a| CustomerAddress {
                street: a.street,
                number: a.number,
                neighborhood: a.neighborhood,
                city: a.city,
                uf: a.uf,
                postal_code: a.postal_code,
                complement: a.complement,
                ranking: a.ranking,
                latitude: a.latitude,
                longitude: a.longitude,
                ddd: a.ddd,
                street_type: a.street_type,
            })
            .collect(),
    }
}

fn normalize(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase();

    let collapsed = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

pub fn date_br_to_iso(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%d/%m/%Y") {
        return Some(date.format("%Y-%m-%d").to_string());
    }

    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%d/%m/%Y %H:%M:%S") {
        return Some(dt.date().format("%Y-%m-%d").to_string());
    }

    None
}

fn cosine_similarity(left: &str, right: &str) -> f64 {
    let left_norm = match normalize(left) {
        Some(value) => value,
        None => return 0.0,
    };
    let right_norm = match normalize(right) {
        Some(value) => value,
        None => return 0.0,
    };

    let left_vector = token_frequency(&left_norm);
    let right_vector = token_frequency(&right_norm);

    let mut dot = 0.0;
    for (token, freq) in &left_vector {
        if let Some(freq_right) = right_vector.get(token) {
            dot += (*freq as f64) * (*freq_right as f64);
        }
    }

    let left_norm = left_vector
        .values()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_norm = right_vector
        .values()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        .sqrt();

    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        (dot / (left_norm * right_norm)).min(1.0)
    }
}

fn token_frequency(input: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for token in input.split_whitespace() {
        *map.entry(token.to_string()).or_insert(0) += 1;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(
            normalize("  José   da Silva  ").as_deref(),
            Some("jose da silva")
        );
        assert_eq!(normalize("   "), None);
    }

    #[test]
    fn test_date_br_to_iso() {
        assert_eq!(date_br_to_iso("02/04/1985"), Some("1985-04-02".into()));
        assert_eq!(
            date_br_to_iso("02/04/1985 10:30:22"),
            Some("1985-04-02".into())
        );
        assert_eq!(date_br_to_iso(""), None);
    }

    #[test]
    fn test_cosine_similarity() {
        let score = cosine_similarity("Maria Joaquina", "Maria de Joaquina");
        assert!(score > 0.5);
        assert!(cosine_similarity("Joao", "Maria") < 0.2);
    }
}
