use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const USER_AGENT: &str = "arc-tui/0.1.0 (aur-commander)";
const AUR_RPC_URL: &str = "https://aur.archlinux.org/rpc/v5";
const AUR_CGIT_URL: &str = "https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=";

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct AurRpcResponse {
    pub version: u32,
    #[serde(rename = "type")]
    pub response_type: String,
    pub resultcount: usize,
    pub results: Vec<AurPackage>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct AurPackage {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "NumVotes")]
    pub votes: i32,
    #[serde(rename = "Popularity")]
    pub popularity: f64,
    #[serde(rename = "Maintainer")]
    pub maintainer: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    #[serde(rename = "OutOfDate")]
    pub out_of_date: Option<i64>,
}

pub struct AurClient {
    agent: ureq::Agent,
}

impl AurClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build();
        Self { agent }
    }

    /// Smart search prioritizing Exact Name match, querying package names only (?by=name)
    pub fn search(&self, query: &str) -> Result<Vec<AurPackage>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }

        let mut results_map: HashMap<String, AurPackage> = HashMap::new();

        // 1. Exact Name match via /info (works for 1-char like 's', 'f', and exact package names)
        let info_url = format!("{}/info?arg[]={}", AUR_RPC_URL, q);
        if let Ok(resp) = self.agent.get(&info_url).call() {
            if let Ok(info_res) = resp.into_json::<AurRpcResponse>() {
                for pkg in info_res.results {
                    results_map.insert(pkg.name.clone(), pkg);
                }
            }
        }

        // 2. Search by package name only (?by=name), completely ignoring descriptions
        if q.chars().count() >= 2 {
            let search_url = format!("{}/search/{}?by=name", AUR_RPC_URL, q);
            if let Ok(resp) = self.agent.get(&search_url).call() {
                if let Ok(search_res) = resp.into_json::<AurRpcResponse>() {
                    for pkg in search_res.results {
                        results_map.entry(pkg.name.clone()).or_insert(pkg);
                    }
                }
            }
        }

        let mut results: Vec<AurPackage> = results_map.into_values().collect();

        // 3. Relevance Ranking:
        // Rank 1: Exact match (e.g. 'aurutils' == 'aurutils')
        // Rank 2: Starts with query (e.g. 'aurutils-git')
        // Rank 3: Alphabetical A-Z
        let q_lower = q.to_lowercase();
        results.sort_by(|a, b| {
            let a_lower = a.name.to_lowercase();
            let b_lower = b.name.to_lowercase();

            let a_exact = a_lower == q_lower;
            let b_exact = b_lower == q_lower;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }

            let a_starts = a_lower.starts_with(&q_lower);
            let b_starts = b_lower.starts_with(&q_lower);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }

            a_lower.cmp(&b_lower)
        });

        Ok(results)
    }

    pub fn info(&self, pkg_names: &[&str]) -> Result<Vec<AurPackage>> {
        if pkg_names.is_empty() {
            return Ok(Vec::new());
        }

        let mut request = self.agent.get(&format!("{}/info", AUR_RPC_URL));
        for name in pkg_names {
            request = request.query("arg[]", name);
        }

        let response: AurRpcResponse = request
            .call()
            .context("AUR info request failed")?
            .into_json()
            .context("Failed to parse AUR info JSON")?;

        Ok(response.results)
    }

    pub fn fetch_pkgbuild(&self, pkg_name: &str) -> Result<String> {
        let url = format!("{}{}", AUR_CGIT_URL, pkg_name);
        let content = self
            .agent
            .get(&url)
            .call()
            .context("Failed to fetch PKGBUILD")?
            .into_string()
            .context("Failed to read PKGBUILD text")?;

        Ok(content)
    }

    pub fn check_updates(&self, local_pkgs: &[(&str, &str)]) -> Result<Vec<(String, String)>> {
        if local_pkgs.is_empty() {
            return Ok(Vec::new());
        }

        let names: Vec<&str> = local_pkgs.iter().map(|(n, _)| *n).collect();
        let aur_info = self.info(&names)?;

        let mut updates = Vec::new();
        for (local_name, local_ver) in local_pkgs {
            if let Some(aur_pkg) = aur_info.iter().find(|p| p.name == *local_name) {
                if alpm::vercmp(aur_pkg.version.as_str(), *local_ver).is_gt() {
                    updates.push((local_name.to_string(), aur_pkg.version.clone()));
                }
            }
        }
        Ok(updates)
    }
}
