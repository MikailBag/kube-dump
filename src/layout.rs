use crate::discover::ApiResource;
use std::path::PathBuf;

/// Layout tells where specific thing should live
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: PathBuf) -> Layout {
        Layout { root }
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

    pub fn object_layout(
        &self,
        resource: &ApiResource,
        namespace: &str,
        name: &str,
    ) -> ObjectLayout {
        let mut p = self.root.clone();
        if namespace.is_empty() {
            p.push("_global_");
        } else {
            p.push(format!("{}", namespace));
        }
        let full_kind = if let Some(group_name) = resource.group_name() {
            format!("{}/{}", group_name, resource.kind)
        } else {
            resource.kind.clone()
        };
        p.push(full_kind);
        p.push(name);

        ObjectLayout { root: p }
    }
}

/// ObjectLayout tells where specific object-related thing should live
pub struct ObjectLayout {
    root: PathBuf,
}

pub enum LogsKind {
    Current,
    Previous
}

impl ObjectLayout {
    pub fn representation(&self) -> PathBuf {
        self.root.join("raw.json")
    }
    pub fn logs(&self, kind: LogsKind, container_name: &str) -> PathBuf {
        let sfx = match kind {
            LogsKind::Current => "",
            LogsKind::Previous => "-prev"
        };
        let file_name = format!("logs-{}{}.txt", container_name,sfx);
        self.root.join(file_name)
    }
}
