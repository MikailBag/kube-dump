mod discover;
mod ext;
mod generic;
mod kubectl;
mod layout;

use anyhow::Context as _;
use clap::Clap;
use ext::ClientExt as _;
use kube::{api::LogParams, Api};
use std::path::PathBuf;

#[derive(Clap)]
struct Opts {
    /// Path dump should be written to
    out: PathBuf,
    /// Strips certain data from dumped object representations.
    /// Supported options (comma-separated):
    /// `managed-fields`: strip `managedFields` from object metadatas (this field usually is
    /// not very helpful and wastes much screen space)
    #[clap(long = "generic-strip")]
    strip: Vec<generic::Strip>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();
    println!("Connecting to cluster");
    let client = kube::Client::try_default()
        .await
        .context("connection failed")?;
    let kube_version = client
        .cluster_version()
        .await
        .context("failed to get kubernetes verion")?;
    println!(
        "successfully connected to Kubernetes v{}.{}",
        kube_version.major, kube_version.minor
    );

    let apis = discover::discover_apis(&client)
        .await
        .context("discovery error")?;
    println!("Discovered {} api resources", apis.len());
    for resource in &apis {
        println!(" - {}/{}", resource.api_group, resource.plural);
    }

    let env = Environment {
        client,
        layout: layout::Layout::new(opts.out.clone()),
        apis,
        opts,
        kubectl: kubectl::Kubectl::try_new().await,
    };
    if let Some(cluster_info) = env.kubectl.exec(&["cluster-info"]).await? {
        tokio::fs::write(env.layout.cluster_info(), cluster_info).await?;
    }
    println!("Running generic dumper");
    generic::dump(&env).await?;
    println!("Running Pods dumper");
    dump_pods(&env).await?;
    Ok(())
}

/// Contains data passed to dumpers
pub struct Environment {
    client: kube::Client,
    layout: layout::Layout,
    apis: Vec<discover::ApiResource>,
    opts: Opts,
    kubectl: kubectl::Kubectl,
}

async fn dump_pods(env: &Environment) -> anyhow::Result<()> {
    let pods_api = Api::<k8s_openapi::api::core::v1::Pod>::all(env.client.clone());
    let pods = pods_api
        .list(&Default::default())
        .await
        .context("failed to list pods")?;
    for pod in pods {
        let pod_meta = pod.metadata.as_ref().unwrap();
        let pod_name = pod_meta.name.as_ref().unwrap();
        let pod_namespace = pod_meta.namespace.as_ref().unwrap();
        let namespaced_pods_api =
            Api::<k8s_openapi::api::core::v1::Pod>::namespaced(env.client.clone(), pod_namespace);
        let object_layout = env.layout.object_layout(
            &discover::ApiResource::from_kube::<k8s_openapi::api::core::v1::Pod>(),
            pod_namespace,
            pod_name,
        );
        // for pod, we will fetch its logs
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
            let current_logs = namespaced_pods_api.logs(pod_name, &log_params).await.ok();
            if let Some(current_logs) = current_logs {
                tokio::fs::write(
                    object_layout.logs(layout::LogsKind::Current, &container.name),
                    current_logs,
                )
                .await?;
            }

            log_params.previous = true;
            let prev_logs = namespaced_pods_api.logs(pod_name, &log_params).await.ok();
            if let Some(prev_logs) = prev_logs {
                tokio::fs::write(
                    object_layout.logs(layout::LogsKind::Previous, &container.name),
                    prev_logs,
                )
                .await?;
            }
        }
    }
    Ok(())
}
