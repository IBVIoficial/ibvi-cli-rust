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
    #[serde(skip)]
    pub from_priority_table: bool,
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
        // Use service role key if available, otherwise use anon key
        let auth_key: &String = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        // First try to fetch from iptus_list_priority
        tracing::info!("Checking iptus_list_priority table for pending jobs...");
        let priority_url: String = format!("{}/rest/v1/iptus_list_priority", self.base_url);

        let priority_response: reqwest::Response = self
            .client
            .get(&priority_url)
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

        if priority_response.status().is_success() {
            let text = priority_response.text().await?;
            tracing::debug!("Response from iptus_list_priority: {}", text);

            let mut priority_jobs =
                serde_json::from_str::<Vec<PendingJob>>(&text).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to parse priority response: {}. Response: {}",
                        e,
                        text
                    )
                })?;

            if !priority_jobs.is_empty() {
                tracing::info!(
                    "Found {} priority jobs in iptus_list_priority",
                    priority_jobs.len()
                );
                // Mark these jobs as coming from priority table
                for job in &mut priority_jobs {
                    job.from_priority_table = true;
                }
                return Ok(priority_jobs);
            } else {
                tracing::info!(
                    "No pending jobs found in iptus_list_priority, checking iptus_list..."
                );
            }
        } else {
            let error_text = priority_response.text().await?;
            tracing::warn!("Could not fetch from iptus_list_priority: {}", error_text);
        }

        // If no priority jobs or priority table doesn't exist, fetch from regular iptus_list
        let url = format!("{}/rest/v1/iptus_list", self.base_url);

        let response = self
            .client
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
            anyhow::bail!(
                "Failed to fetch pending jobs from both tables: {}",
                error_text
            );
        }

        let text = response.text().await?;
        tracing::debug!("Response from iptus_list: {}", text);

        let jobs = serde_json::from_str::<Vec<PendingJob>>(&text)
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}. Response: {}", e, text))?;

        if !jobs.is_empty() {
            tracing::info!("Found {} jobs in iptus_list", jobs.len());
        } else {
            tracing::info!("No pending jobs found in either table");
        }

        Ok(jobs)
    }

    pub async fn claim_jobs(
        &self,
        job_ids: Vec<String>,
        _machine_id: &str,
        from_priority_table: bool,
    ) -> Result<()> {
        let table_name = if from_priority_table {
            "iptus_list_priority"
        } else {
            "iptus_list"
        };
        let url = format!("{}/rest/v1/{}", self.base_url, table_name);

        // Use service role key if available, otherwise use anon key
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update_data = serde_json::json!({
            "status": "p",  // p for processing
        });

        tracing::info!("Claiming {} jobs from {}", job_ids.len(), table_name);

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

        let response = self
            .client
            .post(&url)
            .header("apikey", auth_key)
            .header("Authorization", format!("Bearer {}", auth_key))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates") // Use upsert instead of insert
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

        let response = self
            .client
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

    pub async fn update_batch_progress(
        &self,
        batch_id: &str,
        processados: i32,
        sucesso: i32,
        erros: i32,
    ) -> Result<()> {
        let url = format!("{}/rest/v1/batches", self.base_url);
        let auth_key = self.service_role_key.as_ref().unwrap_or(&self.api_key);

        let update = serde_json::json!({
            "processados": processados,
            "sucesso": sucesso,
            "erros": erros,
        });

        let response = self
            .client
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

    pub async fn mark_iptu_list_as_success(
        &self,
        contributor_numbers: Vec<String>,
        from_priority_table: bool,
    ) -> Result<()> {
        let table_name = if from_priority_table {
            "iptus_list_priority"
        } else {
            "iptus_list"
        };
        let url = format!("{}/rest/v1/{}", self.base_url, table_name);

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

    pub async fn mark_iptu_list_as_error(
        &self,
        contributor_numbers: Vec<String>,
        from_priority_table: bool,
    ) -> Result<()> {
        let table_name = if from_priority_table {
            "iptus_list_priority"
        } else {
            "iptus_list"
        };
        let url = format!("{}/rest/v1/{}", self.base_url, table_name);

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

        let response = self
            .client
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

        let response = self
            .client
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
