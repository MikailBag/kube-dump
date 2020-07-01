use anyhow::Context as _;
use clap::Clap;
use kube::{
    api::{LogParams, Resource},
    Api, Client,
};
use std::path::{Path, PathBuf};

#[tokio::main]
async fn main() {
    if let Err(err) = amain().await {
        eprintln!("Error: {:#}", err);
        std::process::exit(1);
    }
}

#[derive(Clap)]
struct Opts {
    /// Path dump should be written to
    out: PathBuf,
}

async fn amain() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();
    println!("Connecting to cluster");
    let client = kube::Client::try_default()
        .await
        .context("connection failed")?;
    let kube_version: k8s_openapi::apimachinery::pkg::version::Info = client
        .request(http::Request::builder().uri("/version").body(Vec::new())?)
        .await
        .context("failed to get kubernetes verion")?;
    println!(
        "successfully connected to Kubernetes v{}.{}",
        kube_version.major, kube_version.minor
    );

    dump(client, &opts.out).await.context("fatal dump error")?;
    Ok(())
}

fn resource_fully_qualified_name(res: &Resource) -> String {
    format!("{}.{}", res.api_version, res.kind)
}

async fn dump(client: Client, path: &Path) -> anyhow::Result<()> {
    let resources = [
        Resource::all::<k8s_openapi::api::core::v1::Namespace>(),
        Resource::all::<k8s_openapi::api::core::v1::Pod>(),
        Resource::all::<k8s_openapi::api::apps::v1::Deployment>(),
        Resource::all::<k8s_openapi::api::apps::v1::ReplicaSet>(),
        // Resource::all::<k8s_openapi::api::core::v1::ConfigMap>(),
        // Resource::all::<k8s_openapi::api::core::v1::Secret>(),
        Resource::all::<k8s_openapi::api::core::v1::Service>(),
        Resource::all::<k8s_openapi::api::core::v1::Node>(),
        Resource::all::<k8s_openapi::api::batch::v1::Job>(),
        // Resource::all::<k8s_openapi::api::core::v1::PersistentVolume>(),
        // Resource::all::<k8s_openapi::api::core::v1::PersistentVolumeClaim>(),
    ];

    for res in &resources {
        println!("dumping {}", resource_fully_qualified_name(res));
        dump_resources(client.clone(), res.clone(), path.join(&res.kind)).await?;
    }

    Ok(())
}

type ErasedObject = kube::api::Object<serde_json::Value, serde_json::Value>;
type ErasedObjectList = kube::api::ObjectList<ErasedObject>;

fn parse_erased_object_list(mut val: serde_json::Value) -> anyhow::Result<ErasedObjectList> {
    // for some strange reason, k8s returns list in which items are missing
    // apiVersion and kind. Let's workaround it
    {
        let val = &mut val;
        let val = val.as_object_mut().context("not an object")?;
        let val = val
            .get_mut("items")
            .context("items missing")?
            .as_array_mut()
            .context("items is not a list")?;
        for item in val {
            let item = item.as_object_mut().context("item is not an object")?;
            item.insert("apiVersion".to_string(), "hacks.io/v0".into());
            item.insert("kind".to_string(), "Hack".into());
        }
    }
    serde_json::from_value(val).context("parse error")
}

fn parse_erased_object<K: serde::de::DeserializeOwned + k8s_openapi::Resource>(
    mut obj: ErasedObject,
) -> anyhow::Result<K> {
    obj.types.api_version = K::API_VERSION.to_string();
    obj.types.kind = K::KIND.to_string();
    let obj = serde_json::to_string(&obj)?;
    serde_json::from_str(&obj).context("parse error")
}

async fn dump_resources(client: Client, res: Resource, path: PathBuf) -> anyhow::Result<()> {
    tokio::fs::remove_dir_all(&path).await.ok();
    tokio::fs::create_dir(&path).await?;

    // at first, let's enumerate resource
    let list_request = res.list(&Default::default())?;
    let list: serde_json::Value = client.request(list_request).await.context("list error")?;
    let list: ErasedObjectList = parse_erased_object_list(list)?;
    for item in list.items {
        let obj_name = match item.metadata.name.as_deref() {
            Some(name) => name,
            None => {
                eprintln!(
                    "warn: skipping unnamed resource {}",
                    item.metadata.uid.as_deref().unwrap_or("<uid missing too>")
                );
                continue;
            }
        };
        let ns_name = item.metadata.namespace.as_deref().unwrap_or("<global>");
        println!("[generic] Dumping {}/{}", ns_name, obj_name);
        let mut obj_path = path.clone();
        if let Some(ns) = &item.metadata.namespace {
            obj_path.push(ns);
        }
        obj_path.push(obj_name);
        tokio::fs::create_dir_all(&obj_path).await?;

        if let Err(err) = dump_single_resource(&client, &res, item, obj_path).await {
            eprintln!("Warning: dumping failed: {:#}", err);
        }
    }
    Ok(())
}

async fn dump_single_resource(
    client: &Client,
    resource_info: &Resource,
    obj: ErasedObject,
    out: PathBuf,
) -> anyhow::Result<()> {
    let raw_data = serde_json::to_string_pretty(&obj)?;
    tokio::fs::write(out.join("raw.json"), raw_data).await?;
    if resource_info.kind == "Pod" {
        dump_pod(client, obj, out).await?;
    }

    Ok(())
}

async fn dump_pod(client: &Client, obj: ErasedObject, out: PathBuf) -> anyhow::Result<()> {
    let pod: k8s_openapi::api::core::v1::Pod = parse_erased_object(obj)?;
    let pod_name = pod.metadata.as_ref().unwrap().name.as_ref().unwrap();
    let pods_api = Api::<k8s_openapi::api::core::v1::Pod>::namespaced(
        client.clone(),
        pod.metadata.as_ref().unwrap().namespace.as_ref().unwrap(),
    );
    // for pod, we will fetch its logs
    let pod_spec = pod.spec.as_ref().unwrap();
    for container in &pod_spec.containers {
        let container_logs_path = out.join(&container.name);
        tokio::fs::create_dir(&container_logs_path).await?;
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
        let current_logs = pods_api.logs(pod_name, &log_params).await?;

        tokio::fs::write(container_logs_path.join("logs.txt"), current_logs).await?;
        log_params.previous = true;
        let prev_logs = pods_api.logs(pod_name, &log_params).await.ok();
        if let Some(prev_logs) = prev_logs {
            tokio::fs::write(container_logs_path.join("previous-logs.txt"), prev_logs).await?;
        }
    }
    Ok(())
}
