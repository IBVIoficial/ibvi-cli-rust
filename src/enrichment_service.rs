use std::sync::Arc;

use actix_web::{
    error::{ErrorBadGateway, ErrorBadRequest},
    middleware::Logger,
    web, App, HttpResponse, HttpServer, Responder,
};
use anyhow::Result;
use serde::Deserialize;
use tracing::info;

use crate::diretrix_enrichment::{enrich_person, DiretrixClient, EnrichmentRequest};

#[derive(Clone)]
struct AppState {
    client: Arc<DiretrixClient>,
}

#[derive(Debug, Deserialize)]
struct EnrichmentPayload {
    search_types: Vec<String>,
    searches: Vec<String>,
}

impl EnrichmentPayload {
    fn into_request(self) -> Result<EnrichmentRequest, actix_web::Error> {
        if self.search_types.len() != self.searches.len() {
            return Err(ErrorBadRequest(
                "search_types and searches must have same length",
            ));
        }

        let mut cpf: Option<String> = None;
        let mut name: Option<String> = None;
        let mut email: Option<String> = None;
        let mut phone: Option<String> = None;

        for (ty, value) in self.search_types.into_iter().zip(self.searches.into_iter()) {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }

            match ty.to_lowercase().as_str() {
                "cpf" => cpf = Some(trimmed),
                "name" | "nome" => name = Some(trimmed),
                "email" => email = Some(trimmed),
                "phone" | "telefone" => phone = Some(trimmed),
                _ => {
                    return Err(ErrorBadRequest(format!("Unsupported search type: {}", ty)));
                }
            }
        }

        if cpf.is_none() && name.is_none() && email.is_none() && phone.is_none() {
            return Err(ErrorBadRequest(
                "At least one of cpf, name, email, or phone must be provided",
            ));
        }

        Ok(EnrichmentRequest {
            cpf,
            name,
            email,
            phone,
        })
    }
}

async fn enrich_handler(
    state: web::Data<AppState>,
    payload: web::Json<EnrichmentPayload>,
) -> Result<impl Responder, actix_web::Error> {
    let request = payload.into_inner().into_request()?;

    match enrich_person(&state.client, request).await {
        Ok(Some(result)) => Ok(HttpResponse::Ok().json(result)),
        Ok(None) => {
            Ok(HttpResponse::NotFound().json(serde_json::json!({ "message": "Not found" })))
        }
        Err(err) => {
            let message = format!("Diretrix enrichment failed: {}", err);
            Err(ErrorBadGateway(message))
        }
    }
}

pub async fn run_enrichment_server(addr: &str) -> Result<()> {
    let client = DiretrixClient::from_env()?;
    let state = AppState {
        client: Arc::new(client),
    };

    info!("Starting enrichment service on {}", addr);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .wrap(Logger::default())
            .route("/enrich/person", web::post().to(enrich_handler))
    })
    .bind(addr)?
    .run()
    .await?;

    Ok(())
}
