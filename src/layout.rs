use kube::api::ApiResource;
use std::path::PathBuf;

/// Layout tells where specific thing should live
pub struct Layout {
    root: PathBuf,
    escape: bool,
}

impl Layout {
    pub fn new(opts: &crate::Opts) -> Layout {
        Layout {
            root: opts.out.clone(),
            escape: opts.escape_paths,
        }
    }
    /// information, reported by `kubectl cluster-info`
    pub fn cluster_info(&self) -> PathBuf {
        self.root.join("cluster-info.txt")
    }
    /// Kuberntetes release
    pub fn cluster_version(&self) -> PathBuf {
        self.root.join("cluster-version.json")
    }
    /// All discovered API resources
    pub fn cluster_api_resources(&self) -> PathBuf {
        self.root.join("apis.json")
    }

    fn maybe_escape_name(&self, name: &str) -> String {
        if !self.escape {
            return name.to_string();
        }
        name.replace("~", "~tilda_").replace(":", "~colon_")
    }

    pub fn object_layout(
        &self,
        resource: &ApiResource,
        namespace: Option<&str>,
        name: &str,
    ) -> ObjectLayout {
        let mut p = self.root.clone();
        if let Some(ns) = namespace {
            p.push(format!("{}", ns));
        } else {
            p.push("_global_");
        }
        let full_kind = if !resource.group.is_empty() {
            format!("{}/{}", resource.group, resource.kind)
        } else {
            resource.kind.clone()
        };
        p.push(full_kind);
        p.push(self.maybe_escape_name(name));

        ObjectLayout { root: p }
    }
}

/// ObjectLayout tells where specific object-related thing should live
pub struct ObjectLayout {
    root: PathBuf,
}

pub enum LogsKind {
    Current,
    Previous,
}

impl ObjectLayout {
    pub fn representation(&self) -> PathBuf {
        self.root.join("raw.json")
    }
    // for pods
    pub fn logs(&self, kind: LogsKind, container_name: &str) -> PathBuf {
        let sfx = match kind {
            LogsKind::Current => "",
            LogsKind::Previous => "-prev",
        };
        let file_name = format!("logs-{}{}.txt", container_name, sfx);
        self.root.join(file_name)
    }
    // for configmaps and secrets
    pub fn data_piece(&self, key: &str) -> PathBuf {
        self.root.join(format!("data-{}", key))
    }
    pub fn event_log(&self) -> PathBuf {
        self.root.join("events.txt")
    }
}
