//! This module defines utilities to discover all api resources
use crate::ext::ClientExt;
use anyhow::Context;

#[derive(serde::Serialize)]
pub struct ApiResource {
    /// E.g. "apps/v1" or "v1" (for legacy api group)
    pub api_group: String,
    /// E.g. "Deployment"
    pub kind: String,
    /// E.g. "statefulsets", used in urls
    pub plural: String,
}

impl ApiResource {
    fn from_kube_api_resource_list(
        mut list: k8s_openapi::apimachinery::pkg::apis::meta::v1::APIResourceList,
    ) -> Vec<Self> {
        std::mem::take(&mut list.resources)
            .into_iter()
            // let's filter out subresources and not-listable-resources
            .filter(|kube_api_resource| {
                !kube_api_resource.name.contains('/')
                    && kube_api_resource.verbs.iter().any(|verb| verb == "list")
            })
            .map(|kube_api_resource| ApiResource {
                api_group: list.group_version.clone(),
                kind: kube_api_resource.kind,
                plural: kube_api_resource.name,
            })
            .collect()
    }

    pub fn from_kube<K: k8s_openapi::Resource>() -> Self {
        Self {
            api_group: K::API_VERSION.to_string(),
            kind: K::KIND.to_string(),
            // TODO
            plural: K::KIND.to_lowercase() + "s",
        }
    }

    pub fn group_name(&self) -> Option<&str> {
        self.api_group.rsplit('/').nth(1)
    }

    pub fn is_legacy(&self) -> bool {
        !self.api_group.contains('/')
    }
}

/// Discovers all API resources in cluster
pub async fn discover_apis(client: &kube::Client) -> anyhow::Result<Vec<ApiResource>> {
    let grouped_apis = discover_grouped_apis(client).await;
    let legacy_apis = discover_legacy_apis(client).await;
    let mut apis = Vec::new();
    for api_list in vec![grouped_apis, legacy_apis] {
        match api_list {
            Ok(mut a) => apis.append(&mut a),
            Err(err) => {
                eprintln!("Failed to discover apis: {:#}", err);
            }
        }
    }
    Ok(apis)
}

async fn discover_legacy_apis(client: &kube::Client) -> anyhow::Result<Vec<ApiResource>> {
    let versions = client
        .list_legacy_api_versions()
        .await
        .context("failed to list legacy api group versions")?
        .versions;
    if versions
        .iter()
        .map(String::as_str)
        .find(|&vers| vers == "v1")
        .is_some()
    {
        let res_list = client.list_legacy_api_resources("v1").await?;

        Ok(ApiResource::from_kube_api_resource_list(res_list))
    } else {
        anyhow::bail!("legacy v1 not supported by this cluster");
        // TODO do we care?
    }
}

async fn discover_grouped_apis(client: &kube::Client) -> anyhow::Result<Vec<ApiResource>> {
    let groups = client.list_api_groups().await?;
    let mut apis = Vec::new();
    for group in groups.groups {
        let group_name = group.name.clone();
        let mut group_apis = match discover_api_group(client, group).await {
            Ok(a) => a,
            Err(err) => {
                eprintln!(
                    "Warning: failed to discover api group {}: {:#}",
                    group_name, err
                );
                Vec::new()
            }
        };
        apis.append(&mut group_apis)
    }
    Ok(apis)
}

async fn discover_api_group(
    client: &kube::Client,
    group: k8s_openapi::apimachinery::pkg::apis::meta::v1::APIGroup,
) -> anyhow::Result<Vec<ApiResource>> {
    let version = group
        .preferred_version
        .context("preferredVersion missing")?
        .version;
    let resource_list = client
        .list_api_group_resources(&group.name, &version)
        .await?;
    Ok(ApiResource::from_kube_api_resource_list(resource_list))
}
