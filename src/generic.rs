//! Generic dumping behavior
use kube::api::{Api, ApiResource, DynamicObject};

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
        let version = env.client.apiserver_version().await?;
        let version = serde_json::to_string_pretty(&version)?;
        tokio::fs::write(env.layout.cluster_version(), version).await?;
    }
    {
        let apis = env
            .apis
            .iter()
            .map(
                |(
                    ApiResource {
                        group,
                        version,
                        api_version,
                        kind,
                        plural,
                    },
                    _,
                )| {
                    serde_json::json!({
                        "group": group,
                        "version": version,
                        "apiVersion": api_version,
                        "kind": kind,
                        "plural": plural
                    })
                },
            )
            .collect::<Vec<_>>();
        let apis = serde_json::to_string_pretty(&apis)?;
        tokio::fs::write(env.layout.cluster_api_resources(), apis).await?;
    }
    for (api_resource, extras) in &env.apis {
        if !extras.operations.list {
            continue;
        }
        if let Err(err) = dump_api_group(env, api_resource).await {
            eprintln!(
                "Failed to dump {}.{}: {:#}",
                api_resource.api_version, api_resource.kind, err
            );
        }
    }
    Ok(())
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
    println!(" - {}.{}", api_resource.kind, api_resource.api_version);

    let api = Api::<DynamicObject>::all_with(env.client.clone(), api_resource);

    let object_list: Vec<DynamicObject> = api.list(&Default::default()).await?.items;
    for object in object_list {
        let object_layout = env.layout.object_layout(
            api_resource,
            object.metadata.namespace.as_deref(),
            object.metadata.name.as_deref().unwrap(),
        );
        let repr_path = object_layout.representation();
        let mut object = object;
        apply_strips(&mut object.data, &env.opts.strip);
        let repr = serde_json::to_string_pretty(&object)?;
        let parent = repr_path.parent().expect("Layout never returns root-path");
        tokio::fs::create_dir_all(parent).await?;
        tokio::fs::write(repr_path, repr).await?;
    }
    Ok(())
}
