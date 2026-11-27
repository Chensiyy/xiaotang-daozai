use super::{
    appearance_assets,
    cover::{open_plain_file, COVER_LIMIT},
    fail, Result, COVER_FILE,
};
use crate::plugin_canvas::valid_image_path;
use base64::{engine::general_purpose::STANDARD, Engine};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub(super) const IMAGE_LIMIT: u64 = 512 * 1024;
const SIDE_LIMIT: u32 = 1024;
const HISTORY_BUDGET: usize = 32 * 1024 * 1024;
const ACTIVE_BUDGET: usize = 64 * 1024 * 1024;
const TEXTURE_LIMIT: usize = 128;
pub(super) type CanvasImages = BTreeMap<String, CanvasImage>;

#[derive(Clone)]
pub(super) struct CanvasImage {
    bytes: Arc<[u8]>,
    decoded_bytes: Arc<OnceLock<usize>>,
}

struct CachedTexture {
    image: Weak<OnceLock<usize>>,
    used_pass: u64,
    texture: egui::TextureHandle,
}

#[derive(Default)]
struct TextureCache {
    entries: VecDeque<CachedTexture>,
    bytes: usize,
}

impl TextureCache {
    fn for_context(ctx: &egui::Context) -> Arc<Mutex<Self>> {
        let mut install_cleanup = false;
        let cache = ctx.data_mut(|data| {
            Arc::clone(data.get_temp_mut_or_insert_with(
                egui::Id::new("plugin-canvas-textures"),
                || {
                    install_cleanup = true;
                    Arc::<Mutex<Self>>::default()
                },
            ))
        });
        if install_cleanup {
            let cache = Arc::downgrade(&cache);
            ctx.on_end_pass(
                "plugin-canvas-textures",
                Arc::new(move |ctx| {
                    if ctx.viewport_id() == egui::ViewportId::ROOT {
                        if let Some(cache) = cache.upgrade() {
                            let pass = ctx.cumulative_pass_nr();
                            cache
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .finish_pass(pass);
                        }
                    }
                }),
            );
        }
        cache
    }

    fn trim_to(&mut self, bytes: usize, entries: usize, pass: u64) -> bool {
        while self.bytes > bytes || self.entries.len() > entries {
            let Some(oldest) = self.entries.back() else {
                return false;
            };
            if oldest.used_pass == pass {
                return false;
            }
            self.bytes -= self.entries.pop_back().unwrap().texture.byte_size();
        }
        true
    }

    fn finish_pass(&mut self, pass: u64) {
        self.entries.retain(|entry| {
            let keep = entry.used_pass == pass || entry.image.strong_count() != 0;
            if !keep {
                self.bytes -= entry.texture.byte_size();
            }
            keep
        });
        // 本轮仍绘制的图片可以超过历史预算，避免每帧重新解码和上传。
        self.trim_to(HISTORY_BUDGET, TEXTURE_LIMIT, pass);
    }
}

impl CanvasImage {
    pub(super) fn checked(name: &str, bytes: Vec<u8>) -> Result<Self> {
        let image = Self {
            bytes: bytes.into(),
            decoded_bytes: Arc::new(OnceLock::new()),
        };
        image.decode(name)?;
        Ok(image)
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn decode(&self, name: &str) -> Result<egui::ColorImage> {
        let image = if Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            appearance_assets::load_static_svg(
                std::str::from_utf8(&self.bytes)?,
                IMAGE_LIMIT as usize,
                SIDE_LIMIT,
            )?
        } else {
            let mut reader =
                image::ImageReader::with_format(Cursor::new(&self.bytes), image::ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(SIDE_LIMIT);
            limits.max_image_height = Some(SIDE_LIMIT);
            limits.max_alloc = Some(16 * 1024 * 1024);
            reader.limits(limits);
            let rgba = reader.decode()?.into_rgba8();
            egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            )
        };
        let _ = self
            .decoded_bytes
            .set(image.pixels.len() * std::mem::size_of::<egui::Color32>());
        Ok(image)
    }

    pub(super) fn texture(
        &self,
        ctx: &egui::Context,
        name: &str,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        // 岛上画布共用根视口。嵌套视口结束绘制时，也不能回收根视口本轮已用的纹理。
        let pass = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
        let cache = TextureCache::for_context(ctx);
        let mut cache = cache.lock().unwrap_or_else(|poison| poison.into_inner());
        let image_id = Arc::as_ptr(&self.decoded_bytes);
        if let Some(i) = cache
            .entries
            .iter()
            .position(|entry| entry.image.as_ptr() == image_id)
        {
            let mut entry = cache.entries.remove(i).unwrap();
            entry.used_pass = pass;
            let image = (entry.texture.id(), entry.texture.size_vec2());
            cache.entries.push_front(entry);
            return Some(image);
        }

        let decoded = if self.decoded_bytes.get().is_none() {
            Some(self.decode(name).ok()?)
        } else {
            None
        };
        let bytes = *self.decoded_bytes.get()?;
        if !cache.trim_to(ACTIVE_BUDGET.checked_sub(bytes)?, TEXTURE_LIMIT - 1, pass) {
            return None;
        }
        let image = decoded.or_else(|| self.decode(name).ok())?;
        let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
        let image = (texture.id(), texture.size_vec2());
        cache.bytes += bytes;
        cache.entries.push_front(CachedTexture {
            image: Arc::downgrade(&self.decoded_bytes),
            used_pass: pass,
            texture,
        });
        Some(image)
    }
}

impl Serialize for CanvasImage {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for CanvasImage {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() as u64 > IMAGE_LIMIT.div_ceil(3) * 4 {
            return Err(Error::custom("插件画布图片超过 512 KiB"));
        }
        let bytes = STANDARD.decode(encoded).map_err(Error::custom)?;
        Ok(Self {
            bytes: bytes.into(),
            decoded_bytes: Arc::new(OnceLock::new()),
        })
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png") || ext.eq_ignore_ascii_case("svg"))
}

pub(super) fn names(base: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut folders = vec![base.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() && path.canonicalize()? == path {
                folders.push(path);
            } else if is_image(&path) && path.is_file() {
                let name = path
                    .strip_prefix(base)?
                    .to_string_lossy()
                    .replace('\\', "/");
                if !valid_image_path(&name) {
                    return Err(fail(&format!("插件画布图片路径无效：{name}")));
                }
                names.push(name);
            }
        }
    }
    Ok(names)
}

pub(super) fn read(base: &Path) -> Result<CanvasImages> {
    decode_all(names(base)?, |name, remaining| {
        let mut bytes = Vec::new();
        open_plain_file(&base.join(name), "插件画布图片必须是包内普通文件")?
            .take(remaining + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

pub(super) fn decode_all(
    mut names: Vec<String>,
    mut load: impl FnMut(&str, u64) -> Result<Vec<u8>>,
) -> Result<CanvasImages> {
    let mut remaining = IMAGE_LIMIT;
    let mut images = BTreeMap::new();
    // 封面是集市的独立资源，先给专用画布图片留出额度。
    names.sort_by_key(|name| name == COVER_FILE);
    for name in names {
        if !valid_image_path(&name) {
            return Err(fail(&format!("插件画布图片路径无效：{name}")));
        }
        let limit = if name == COVER_FILE {
            COVER_LIMIT
        } else {
            remaining
        };
        let bytes = load(&name, limit)?;
        if bytes.len() as u64 > limit {
            return Err(fail("插件画布图片合计超过 512 KiB"));
        }
        if name == COVER_FILE && bytes.len() as u64 > remaining {
            continue;
        }
        let image = match CanvasImage::checked(&name, bytes) {
            Ok(image) => image,
            // 合规的集市封面可能超过画布图元的尺寸。
            Err(_) if name == COVER_FILE => continue,
            Err(error) => return Err(error),
        };
        remaining -= image.bytes().len() as u64;
        images.insert(name, image);
    }
    Ok(images)
}

pub(super) fn valid_snapshot(images: &CanvasImages) -> bool {
    images
        .values()
        .map(|image| image.bytes.len() as u64)
        .sum::<u64>()
        <= IMAGE_LIMIT
        && images
            .iter()
            .all(|(name, image)| valid_image_path(name) && image.decode(name).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_svg(side: u32, color: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" width="{side}" height="{side}"><rect width="{side}" height="{side}" fill="{color}"/></svg>"#
        )
        .into_bytes()
    }

    #[test]
    fn canvas_texture_budget_keeps_used_images_alive_until_the_next_pass() {
        let ctx = egui::Context::default();
        let names: Vec<_> = (0..17).map(|i| format!("image-{i}.svg")).collect();
        let images = decode_all(names.clone(), |_, _| Ok(solid_svg(1024, "#e03030"))).unwrap();
        let mut first = None;
        let mut second = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for name in &names[..16] {
                let id = images[name].texture(ctx, name).unwrap().0;
                if first.is_none() {
                    first = Some(id);
                } else if second.is_none() {
                    second = Some(id);
                }
            }
            let next = &names[16];
            assert!(
                images[next].texture(ctx, next).is_none(),
                "本轮已经画过的图片不能被腾出"
            );
        });
        let first = first.unwrap();
        let second = second.unwrap();
        assert!(ctx.tex_manager().read().meta(first).is_some());
        assert!(ctx.tex_manager().read().meta(second).is_some());

        let out = ctx.run(egui::RawInput::default(), |ctx| {
            assert_eq!(images[&names[0]].texture(ctx, &names[0]).unwrap().0, first);
            assert!(images[&names[16]].texture(ctx, &names[16]).is_some());
        });
        assert!(
            out.textures_delta.free.contains(&second),
            "换轮播图时释放最久没用的纹理"
        );
        assert!(!out.textures_delta.free.contains(&first));
    }

    #[test]
    fn canvas_texture_cache_reuses_active_images_and_trims_when_hidden() {
        let ctx = egui::Context::default();
        let names: Vec<_> = (0..9).map(|i| format!("image-{i}.svg")).collect();
        let images = decode_all(names.clone(), |_, _| Ok(solid_svg(1024, "#e03030"))).unwrap();
        let mut drawn = Vec::new();
        for _ in 0..2 {
            let mut frame = Vec::new();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                frame = names
                    .iter()
                    .map(|name| images[name].texture(ctx, name).unwrap().0)
                    .collect();
            });
            if drawn.is_empty() {
                drawn = frame;
            } else {
                assert_eq!(frame, drawn, "超过历史预算的可见图片仍应复用");
            }
        }
        let hidden = ctx.run(egui::RawInput::default(), |_| {});
        assert!(hidden.textures_delta.free.contains(&drawn[0]));
        assert_eq!(
            drawn
                .iter()
                .filter(|id| ctx.tex_manager().read().meta(**id).is_some())
                .count(),
            8
        );
    }

    #[test]
    fn canvas_texture_cache_uses_the_current_context_and_image_revision() {
        let draw = |ctx: &egui::Context, image: &CanvasImage| {
            let mut id = None;
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                id = image.texture(ctx, "same.svg").map(|texture| texture.0);
            });
            (id.unwrap(), out)
        };
        let first_ctx = egui::Context::default();
        let second_ctx = egui::Context::default();
        let original = CanvasImage::checked("same.svg", solid_svg(2, "#ff0000")).unwrap();
        let replacement = CanvasImage::checked("same.svg", solid_svg(2, "#0000ff")).unwrap();
        let (first_id, _) = draw(&first_ctx, &original);
        let (second_id, second_out) = draw(&second_ctx, &original);
        assert!(second_out
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| *id == second_id));
        assert_eq!(draw(&first_ctx, &original.clone()).0, first_id);
        let (new_id, replaced) = draw(&first_ctx, &replacement);
        assert_ne!(new_id, first_id);
        let img = &replaced
            .textures_delta
            .set
            .iter()
            .find(|(id, _)| *id == new_id)
            .unwrap()
            .1
            .image;
        let egui::ImageData::Color(img) = img else {
            panic!("应上传新资源")
        };
        assert_eq!(img.pixels[0], egui::Color32::BLUE);
        drop(original);
        let released = first_ctx.run(egui::RawInput::default(), |_| {});
        assert!(released.textures_delta.free.contains(&first_id));
        assert!(!released.textures_delta.free.contains(&new_id));
    }

    #[test]
    fn canvas_images_check_dimensions_and_total_bytes() {
        let png = |width| {
            let mut output = Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(width, 1)
                .write_to(&mut output, image::ImageFormat::Png)
                .unwrap();
            output.into_inner()
        };
        assert!(CanvasImage::checked("image.png", png(1024)).is_ok());
        assert!(CanvasImage::checked("image.png", png(1025)).is_err());

        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#000"/></svg>"##;
        let mut bytes = svg.to_vec();
        bytes.resize(IMAGE_LIMIT as usize / 2 + 1, b' ');
        assert!(decode_all(vec!["first.svg".into()], |_, _| Ok(bytes.clone())).is_ok());
        assert!(
            decode_all(vec!["first.svg".into(), "second.svg".into()], |_, _| Ok(
                bytes.clone()
            ))
            .is_err()
        );
        assert!(decode_all(vec!["../other.svg".into()], |_, _| panic!(
            "越界路径不得交给读取器"
        ))
        .is_err());
    }
}
