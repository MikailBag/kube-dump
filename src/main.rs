mod generic;
mod kubectl;
mod layout;

use self::layout::ObjectLayout;

use anyhow::Context as _;
use clap::Clap;
use k8s_openapi::api::core::v1::{ConfigMap, Event, Pod, Secret};
use kube::{
    api::{Api, ApiResource, LogParams, Resource, ResourceExt},
    discovery::{ApiCapabilities, Discovery},
};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, fmt::Debug, future::Future, path::PathBuf, sync::Arc};

#[derive(Clap)]
pub struct Opts {
    /// Path dump should be written to
    out: PathBuf,
    /// Strips certain data from dumped object representations.
    /// Supported options (comma-separated):
    /// `managed-fields`: strip `managedFields` from object metadatas (this field usually is
    /// not very helpful and wastes much screen space)
    #[clap(long = "generic-strip")]
    strip: Vec<generic::Strip>,
    /// Escape some chars in names
    #[clap(long)]
    escape_paths: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();
    println!("Connecting to cluster");
    let client = kube::Client::try_default()
        .await
        .context("connection failed")?;
    let kube_version = client
        .apiserver_version()
        .await
        .context("failed to get kubernetes verion")?;
    println!(
        "successfully connected to Kubernetes v{}.{}",
        kube_version.major, kube_version.minor
    );

    let apis = discover_apis(&client).await.context("discovery error")?;
    println!("Discovered {} api resources", apis.len());

    let env = Environment {
        client,
        layout: layout::Layout::new(&opts),
        apis,
        opts,
        kubectl: kubectl::Kubectl::try_new().await,
    };
    if let Some(cluster_info) = env.kubectl.exec(&["cluster-info"]).await? {
        tokio::fs::write(env.layout.cluster_info(), cluster_info).await?;
    }
    println!("Running generic dumper");
    generic::dump(&env).await?;
    let env = Arc::new(env);
    println!("Running Pod dumper");
    dump_typed_simple(dump_pod, &env).await?;
    println!("Running ConfigMap dumper");
    dump_typed_simple(dump_config_map, &env).await?;
    println!("Running Secret dumper");
    dump_typed_simple(dump_secret, &env).await?;
    println!("Running Event dumper");
    dump_events(&env).await?;
    Ok(())
}

async fn discover_apis(k: &kube::Client) -> anyhow::Result<Vec<(ApiResource, ApiCapabilities)>> {
    let discovery = Discovery::new(k.clone()).run().await?;
    let mut res = Vec::new();
    for g in discovery.groups() {
        let v = g.preferred_version_or_latest();
        let mut resources = g.versioned_resources(v).into_iter().collect();
        res.append(&mut resources);
    }
    Ok(res)
}

/// Contains data passed to dumpers
pub struct Environment {
    client: kube::Client,
    layout: layout::Layout,
    apis: Vec<(ApiResource, ApiCapabilities)>,
    opts: Opts,
    kubectl: kubectl::Kubectl,
}

async fn dump_typed_simple<K, F, Fut>(func: F, env: &Arc<Environment>) -> anyhow::Result<()>
where
    K: Resource<DynamicType = ()> + Clone + DeserializeOwned + Debug,
    F: Fn(K, Arc<Environment>, ObjectLayout) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let api = Api::<K>::all(env.client.clone());
    let objects = api
        .list(&Default::default())
        .await
        .context("failed to list pods")?;
    for obj in objects {
        let name = obj.name();
        let namespace = obj.namespace();
        let object_layout =
            env.layout
                .object_layout(&ApiResource::erase::<K>(&()), namespace.as_deref(), &name);
        func(obj, env.clone(), object_layout)
            .await
            .with_context(|| format!("failed to dump object {:?}/{}", namespace, name))?;
    }
    Ok(())
}

async fn dump_pod(pod: Pod, env: Arc<Environment>, layout: ObjectLayout) -> anyhow::Result<()> {
    let pod_name = pod.name();
    let pod_namespace = pod.namespace().unwrap();
    let namespaced_pods_api = Api::<Pod>::namespaced(env.client.clone(), &pod_namespace);
    let pod_spec = pod.spec.as_ref().unwrap();
    for container in &pod_spec.containers {
        let mut log_params = LogParams {
            container: Some(container.name.clone()),
            follow: false,
            pretty: true,
            previous: false,
            since_seconds: None,
            tail_lines: None,
            timestamps: true,
            limit_bytes: None,
        };
        let current_logs = namespaced_pods_api.logs(&pod_name, &log_params).await.ok();
        if let Some(current_logs) = current_logs {
            tokio::fs::write(
                layout.logs(layout::LogsKind::Current, &container.name),
                current_logs,
            )
            .await?;
        }

        log_params.previous = true;
        let prev_logs = namespaced_pods_api.logs(&pod_name, &log_params).await.ok();
        if let Some(prev_logs) = prev_logs {
            tokio::fs::write(
                layout.logs(layout::LogsKind::Previous, &container.name),
                prev_logs,
            )
            .await?;
        }
    }

    Ok(())
}

async fn dump_config_map(
    cmap: ConfigMap,
    _env: Arc<Environment>,
    layout: ObjectLayout,
) -> anyhow::Result<()> {
    for (key, value) in cmap.binary_data {
        tokio::fs::write(layout.data_piece(&key), value.0).await?;
    }

    for (key, value) in cmap.data {
        let path = layout.data_piece(&key);
        tokio::fs::write(&path, value)
            .await
            .with_context(|| format!("Failed to write to {}", path.display()))?;
    }

    Ok(())
}

async fn dump_secret(
    secret: Secret,
    _env: Arc<Environment>,
    layout: ObjectLayout,
) -> anyhow::Result<()> {
    for (key, value) in secret.data {
        tokio::fs::write(layout.data_piece(&key), value.0).await?;
    }

    Ok(())
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct InvolvedObject {
    group: Option<String>,
    kind: String,
    namespace: Option<String>,
    name: String,
}

impl InvolvedObject {
    fn from_event(ev: &Event) -> Option<Self> {
        let obj = InvolvedObject {
            group: ev
                .involved_object
                .api_version
                .as_deref()
                .unwrap_or("v1")
                .rsplitn(2, '/')
                .nth(1)
                .map(ToString::to_string),
            namespace: ev.involved_object.namespace.clone(),
            name: ev.involved_object.name.clone()?,
            kind: ev.involved_object.kind.clone()?,
        };
        Some(obj)
    }
}

fn event_to_string(ev: Event) -> String {
    // TODO improve
    ev.message.unwrap_or_default()
}

async fn dump_events(env: &Environment) -> anyhow::Result<()> {
    let events_api = Api::<Event>::all(env.client.clone());
    let events = events_api.list(&Default::default()).await?.items;

    let mut mapping = BTreeMap::new();
    for event in events {
        let obj = match InvolvedObject::from_event(&event) {
            Some(o) => o,
            None => {
                eprintln!(
                    "Skipping dangling event {}/{}",
                    event.namespace().unwrap(),
                    event.name()
                );
                continue;
            }
        };
        mapping.entry(obj).or_insert_with(Vec::new).push(event);
    }
    for (object, events) in mapping {
        let resource = ApiResource {
            kind: object.kind,
            group: object.group.unwrap_or_default(),
            api_version: "BUG".to_string(),
            version: "BUG".to_string(),
            plural: "BUG".to_string(),
        };
        let layout = env
            .layout
            .object_layout(&resource, object.namespace.as_deref(), &object.name);
        let repr_path = layout.representation();
        let exists = tokio::task::spawn_blocking(move || repr_path.exists())
            .await
            .unwrap();
        if !exists {
            eprintln!("Skipping event referencing not existing object");
            continue;
        }
        let log = events
            .into_iter()
            .map(event_to_string)
            .collect::<Vec<_>>()
            .join("\n");

        let path = layout.event_log();
        tokio::fs::write(path, log).await?;
    }
    Ok(())
}
