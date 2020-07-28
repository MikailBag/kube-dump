//! Generic dumping behavior
use crate::discover::ApiResource;
use crate::ext::ClientExt as _;

pub enum Strip {
    ManagedFields,
}

impl std::str::FromStr for Strip {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "managed-fields" => Ok(Strip::ManagedFields),
            _ => anyhow::bail!("unknown strip request: {}", s),
        }
    }
}

pub async fn dump(env: &crate::Environment) -> anyhow::Result<()> {
    // dump cluster-wide information
    {
        let version = env.client.cluster_version().await?;
        let version = serde_json::to_string_pretty(&version)?;
        tokio::fs::write(env.layout.cluster_version(), version).await?;
    }
    {
        let apis = serde_json::to_string_pretty(&env.apis)?;
        tokio::fs::write(env.layout.cluster_api_resources(), apis).await?;
    }
    for api_resource in &env.apis {
        if let Err(err) = dump_api_group(env, api_resource).await {
            eprintln!(
                "Failed to dump {}.{}: {:#}",
                api_resource.api_group, api_resource.kind, err
            );
        }
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct ObjectList {
    items: Vec<serde_json::Value>,
}
#[derive(serde::Deserialize)]
struct Object {
    metadata: ObjectMeta,
}
#[derive(serde::Deserialize)]
struct ObjectMeta {
    name: String,
    #[serde(default)]
    namespace: String,
}

/// Modifies `object` in-place, applying all requested strips
fn apply_strips(object: &mut serde_json::Value, strips: &[Strip]) {
    for strip in strips {
        match strip {
            Strip::ManagedFields => {
                if let Some(managed_fields) = object.pointer_mut("/metadata/managedFields") {
                    *managed_fields = serde_json::Value::Null;
                }
            }
        }
    }
}

async fn dump_api_group(
    env: &crate::Environment,
    api_resource: &ApiResource,
) -> anyhow::Result<()> {
    let url = if api_resource.is_legacy() {
        format!("/api/{}/{}", api_resource.api_group, api_resource.plural)
    } else {
        format!("/apis/{}/{}", api_resource.api_group, api_resource.plural)
    };
    let object_list: ObjectList = env
        .client
        .request(http::Request::builder().uri(url).body(Vec::new())?)
        .await?;
    for object in object_list.items {
        let object_info: Object = serde_json::from_value(object.clone())?;
        let object_layout = env.layout.object_layout(
            api_resource,
            &object_info.metadata.namespace,
            &object_info.metadata.name,
        );
        let repr_path = object_layout.representation();
        let mut object = object;
        apply_strips(&mut object, &env.opts.strip);
        let repr = serde_json::to_string_pretty(&object)?;
        let parent = repr_path.parent().expect("Layout never returns root-path");
        tokio::fs::create_dir_all(parent).await?;
        tokio::fs::write(repr_path, repr).await?;
    }
    Ok(())
}
