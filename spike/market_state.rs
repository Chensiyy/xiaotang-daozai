use super::{market, *};
use std::{cmp::Ordering, thread::JoinHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MarketStatus {
    Install,
    Update,
    Installed,
    OtherSource,
    Unavailable,
}

pub(super) struct MarketJob {
    id: String,
    update: bool,
    cancelled: bool,
    result: JoinHandle<std::result::Result<Record, String>>,
}

impl LocalPlugins {
    fn saved_market_path(&self) -> Option<PathBuf> {
        let json = bounded(&self.dir.join("market-catalog.json"), 128 * 1024).ok()?;
        serde_json::from_str(&json).ok()
    }

    pub(super) fn restore_market_catalog(&mut self) {
        if let Some(path) = self.saved_market_path() {
            match market::Catalog::open(&path) {
                Ok(catalog) => self.market_catalog = Some(catalog),
                Err(error) if self.notice.is_empty() => self.notice = error.to_string(),
                Err(_) => {}
            }
        }
    }

    pub(super) fn set_market_catalog(&mut self, path: &Path) -> Result<()> {
        if self.market_busy() {
            return Err(fail("请等待当前集市操作完成"));
        }
        let catalog = market::Catalog::open(path)?;
        crate::model::atomic_write(
            &self.dir.join("market-catalog.json"),
            &serde_json::to_vec(&catalog.path)?,
        )?;
        self.market_catalog = Some(catalog);
        Ok(())
    }

    pub(super) fn refresh_market_catalog(&mut self) -> Result<()> {
        if self.market_busy() {
            return Err(fail("请等待当前集市操作完成"));
        }
        let path = self
            .market_catalog
            .as_ref()
            .map(|catalog| catalog.path.clone())
            .or_else(|| self.saved_market_path())
            .ok_or_else(|| fail("请先选择测试集市目录"))?;
        self.market_catalog = Some(market::Catalog::open(&path)?);
        Ok(())
    }

    pub(super) fn market_status(&self, entry: &market::Entry) -> MarketStatus {
        let Some(catalog) = &self.market_catalog else {
            return MarketStatus::Unavailable;
        };
        let Some(record) = self.records.get(&entry.id) else {
            return MarketStatus::Install;
        };
        if !record.from_market || record.origin != catalog.path {
            return MarketStatus::OtherSource;
        }
        let Some(package) = &record.package else {
            return MarketStatus::Install;
        };
        match market::compare_versions(&entry.version, &package.manifest.version) {
            Some(Ordering::Greater) => MarketStatus::Update,
            Some(Ordering::Equal) => MarketStatus::Installed,
            _ => MarketStatus::Unavailable,
        }
    }

    pub(super) fn market_busy(&self) -> bool {
        self.market_job.is_some()
    }

    pub(super) fn cancel_market(&mut self, id: &str) {
        if let Some(job) = self.market_job.as_mut().filter(|job| job.id == id) {
            job.cancelled = true;
        }
    }

    pub(super) fn market_action(&mut self, id: &str) -> Result<()> {
        if self.market_busy() {
            return Err(fail("请等待当前集市操作完成"));
        }
        let catalog = self
            .market_catalog
            .as_ref()
            .ok_or_else(|| fail("请先选择测试集市目录"))?;
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| fail("测试集市没有这个插件"))?;
        let update = match self.market_status(entry) {
            MarketStatus::Install => false,
            MarketStatus::Update => true,
            MarketStatus::Installed | MarketStatus::Unavailable => {
                return Err(fail("测试集市没有更高版本"));
            }
            MarketStatus::OtherSource => return Err(fail("请从原来源管理这个插件")),
        };
        let record = if let Some(record) = self.records.get(id) {
            record.clone()
        } else {
            if self.path(id).exists() && !self.invalid.contains(id) {
                return Err(fail("已有无法读取的插件记录"));
            }
            Record {
                origin: catalog.path.clone(),
                from_market: true,
                package: None,
                enabled: false,
                values: Values::new(),
                storage: Value::Null,
                material_granted: false,
            }
        };
        let (catalog, entry) = (catalog.clone(), entry.clone());
        self.market_job = Some(MarketJob {
            id: id.to_owned(),
            update,
            cancelled: false,
            result: std::thread::spawn(move || {
                let prepare = || prepare_record(record, catalog.load(&entry)?);
                prepare().map_err(|error| error.to_string())
            }),
        });
        Ok(())
    }

    pub(super) fn poll_market(&mut self) {
        if !self
            .market_job
            .as_ref()
            .is_some_and(|job| job.result.is_finished())
        {
            return;
        }
        let job = self.market_job.take().unwrap();
        if job.cancelled {
            self.notice = "插件状态已变化，请重新操作".into();
            return;
        }
        let result = job
            .result
            .join()
            .unwrap_or_else(|_| Err("集市校验线程退出".into()))
            .and_then(|mut record| {
                // 校验期间，旧插件可能完成一次后台刷新，保留该次已保存的数据。
                if let Some(current) = self.records.get(&job.id) {
                    record.storage = current.storage.clone();
                }
                self.put(&job.id, record)
                    .map_err(|error| error.to_string())?;
                self.invalid.remove(&job.id);
                self.forget_view(&job.id);
                Ok(())
            });
        self.notice = match result {
            Ok(()) if job.update => "已更新，设置和数据已保留".into(),
            Ok(()) => "已安装。启用后开始运行".into(),
            Err(error) if job.update => format!("更新失败，原版本仍可用。{error}"),
            Err(error) => format!("安装失败。{error}"),
        };
    }
}

fn prepare_record(mut record: Record, package: Package) -> Result<Record> {
    let manifest = &package.manifest;
    if let Some(old) = &record.package {
        let old = &old.manifest;
        if old.runtime != manifest.runtime
            || old.owner != manifest.owner
            || old.permissions != manifest.permissions
            || old.data_version != manifest.data_version
            || old
                .capabilities
                .iter()
                .any(|capability| !manifest.capabilities.contains(capability))
        {
            return Err(fail("新版本与已安装版本的能力、权限或数据格式不兼容"));
        }
        if !old.settings.iter().all(|previous| {
            manifest
                .settings
                .iter()
                .any(|field| field.key == previous.key && field.kind == previous.kind)
        }) {
            return Err(fail("新版本删除或更改了已设置的字段类型"));
        }
    }
    let values = package.settings_values(&record.values);
    if record
        .values
        .iter()
        .any(|(key, value)| values.get(key) != Some(value))
    {
        return Err(fail("新版本与已保存的设置不兼容"));
    }
    package.render(
        &values,
        &record.storage,
        &chrono::Local::now().date_naive().to_string(),
    )?;
    record.values = values;
    record.package = Some(package);
    Ok(record)
}
