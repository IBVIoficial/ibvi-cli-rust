use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct PendingJob {
    pub contributor_number: String,
    pub status: Option<String>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub batch_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IPTUResult {
    pub id: Option<String>,
    pub contributor_number: String,
    pub numero_cadastro: Option<String>,
    pub nome_proprietario: Option<String>,
    pub nome_compromissario: Option<String>,
    pub endereco: Option<String>,
    pub numero: Option<String>,
    pub complemento: Option<String>,
    pub bairro: Option<String>,
    pub cep: Option<String>,
    pub sucesso: bool,
    pub erro: Option<String>,
    pub batch_id: Option<String>,
    pub timestamp: String,
    pub processed_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Batch {
    pub id: String,
    pub total: i32,
    pub processados: i32,
    pub sucesso: i32,
    pub erros: i32,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

pub struct SupabaseClient {
    client: Client,
    base_url: String,
    api_key: String,
    service_role_key: Option<String>,
}

impl SupabaseClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            api_key,
            service_role_key: None,
        }
    }

    pub fn with_service_role(mut self, service_role_key: String) -> Self {
        self.service_role_key = Some(service_role_key);
        self
    }

    pub async fn fetch_pending_jobs(&self, limit: usize) -> Result<Vec<PendingJob>> {
        let url = format!("{}/rest/v1/iptus_list", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let response = self.client
            .get(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .query(&[
                ("select", "contributor_number,status"),
                ("status", "is.null"),
                ("order", "contributor_number.asc"),
                ("limit", &limit.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to fetch pending jobs: {}", error_text);
        }

        let text = response.text().await?;
        tracing::debug!("Response from iptu_list: {}", text);

        let jobs = serde_json::from_str::<Vec<PendingJob>>(&text)
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}. Response: {}", e, text))?;

        Ok(jobs)
    }

    pub async fn claim_jobs(&self, job_ids: Vec<String>, _machine_id: &str) -> Result<()> {
        let url = format!("{}/rest/v1/iptus_list", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update_data = serde_json::json!({
            "status": "p",  // p for processing
        });

        for id in job_ids {
            self.client
                .patch(&url)
                .header("apikey", auth_key)
                .header("Authorization", format!("Bearer {}", auth_key))
                .header("Content-Type", "application/json")
                .query(&[("contributor_number", format!("eq.{}", id))])
                .json(&update_data)
                .send()
                .await?;
        }

        Ok(())
    }

    pub async fn upload_results(&self, results: Vec<IPTUResult>) -> Result<usize> {
        let url = format!("{}/rest/v1/iptus", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let response = self.client
            .post(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates")  // Use upsert instead of insert
            .json(&results)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to upload results: {}", error_text);
        }

        Ok(results.len())
    }

    pub async fn create_batch(&self, total: i32) -> Result<String> {
        let url = format!("{}/rest/v1/batches", self.base_url);
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let batch_id = uuid::Uuid::new_v4().to_string();

        // Create batch without created_at field (Supabase will auto-generate it)
        let batch_data = serde_json::json!({
            "id": batch_id,
            "total": total,
            "processados": 0,
            "sucesso": 0,
            "erros": 0,
            "status": "processing"
        });

        let response = self.client
            .post(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .header("Content-Type", "application/json")
            .json(&batch_data)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to create batch: {}", error_text);
        }

        Ok(batch_id)
    }

    pub async fn update_batch_progress(&self, batch_id: &str, processados: i32, sucesso: i32, erros: i32) -> Result<()> {
        let url = format!("{}/rest/v1/batches", self.base_url);
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update = serde_json::json!({
            "processados": processados,
            "sucesso": sucesso,
            "erros": erros,
        });

        let response = self.client
            .patch(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .header("Content-Type", "application/json")
            .query(&[("id", format!("eq.{}", batch_id))])
            .json(&update)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to update batch: {}", error_text);
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn update_job_status(&self, job_id: &str, status: &str) -> Result<()> {
        let url = format!("{}/rest/v1/iptu_queue", self.base_url);

        let update_data = serde_json::json!({
            "status": status,
            "completed_at": chrono::Utc::now().to_rfc3339(),
        });

        self.client
            .patch(&url)
            .header("apikey", &self.api_key)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .query(&[("id", format!("eq.{}", job_id))])
            .json(&update_data)
            .send()
            .await?;

        Ok(())
    }

    pub async fn mark_iptu_list_as_success(&self, contributor_numbers: Vec<String>) -> Result<()> {
        let url = format!("{}/rest/v1/iptus_list", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update_data = serde_json::json!({
            "status": "s",  // s for success
        });

        for number in contributor_numbers {
            self.client
                .patch(&url)
                .header("apikey", auth_key)
                .header("Authorization", format!("Bearer {}", auth_key))
                .header("Content-Type", "application/json")
                .query(&[("contributor_number", format!("eq.{}", number))])
                .json(&update_data)
                .send()
                .await?;
        }

        Ok(())
    }

    pub async fn mark_iptu_list_as_error(&self, contributor_numbers: Vec<String>) -> Result<()> {
        let url = format!("{}/rest/v1/iptus_list", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update_data = serde_json::json!({
            "status": "e",  // e for error
        });

        for number in contributor_numbers {
            self.client
                .patch(&url)
                .header("apikey", auth_key)
                .header("Authorization", format!("Bearer {}", auth_key))
                .header("Content-Type", "application/json")
                .query(&[("contributor_number", format!("eq.{}", number))])
                .json(&update_data)
                .send()
                .await?;
        }

        Ok(())
    }

    pub async fn get_results(&self, limit: i32, offset: i32) -> Result<Vec<IPTUResult>> {
        let url = format!("{}/rest/v1/iptus", self.base_url);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let response = self.client
            .get(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .query(&[
                ("select", "*"),
                ("order", "timestamp.desc"),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to fetch results: {}", error_text);
        }

        let results = response.json::<Vec<IPTUResult>>().await?;
        Ok(results)
    }

    pub async fn complete_batch(&self, batch_id: &str) -> Result<()> {
        let url = format!("{}/rest/v1/batches", self.base_url);
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update = serde_json::json!({
            "status": "completed",
            "completed_at": chrono::Utc::now().to_rfc3339(),
        });

        let response = self.client
            .patch(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .header("Content-Type", "application/json")
            .query(&[("id", format!("eq.{}", batch_id))])
            .json(&update)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to complete batch: {}", error_text);
        }

        Ok(())
    }
}
