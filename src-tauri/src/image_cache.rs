//! 下载并复用推荐物品图片的本地缓存。

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use reqwest::header::CONTENT_TYPE;

use crate::{app_log, models::CraftRecommendation};

const IMAGE_CACHE_DIR_NAME: &str = "images";
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// 管理推荐物品图片的下载和磁盘缓存。
pub(crate) struct ImageCache {
    client: reqwest::blocking::Client,
    cache_dir: PathBuf,
}

impl ImageCache {
    /// 在应用数据目录下创建图片缓存管理器。
    pub(crate) fn new(app_data_dir: &Path) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            cache_dir: app_data_dir.join(IMAGE_CACHE_DIR_NAME),
        }
    }

    /// 将推荐列表中的远程图片替换为可用的本地缓存路径。
    pub(crate) fn cache_recommendation_images(&self, recommendations: &mut [CraftRecommendation]) {
        for recommendation in recommendations {
            let Some(remote_url) = recommendation.image_url.clone() else {
                continue;
            };

            match self.cache_image(&recommendation.id, &remote_url) {
                Ok(path) => recommendation.image_url = Some(path.to_string_lossy().into_owned()),
                Err(error) => {
                    app_log::warn(format!(
                        "物品图片缓存失败，将显示文字占位：{}，{}",
                        recommendation.item_name, error
                    ));
                    recommendation.image_url = None;
                }
            }
        }
    }

    /// 返回现有缓存，或下载并写入指定图片。
    fn cache_image(&self, item_id: &str, remote_url: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.cache_dir).map_err(|error| error.to_string())?;
        let path = self.cache_path(item_id, remote_url);
        if path.exists() {
            match validate_cached_file(&path) {
                Ok(()) => return Ok(path),
                Err(error) => {
                    app_log::warn(format!(
                        "图片缓存损坏，将删除后重新下载：{}，{}",
                        path.display(),
                        error
                    ));
                    fs::remove_file(&path).map_err(|remove_error| remove_error.to_string())?;
                }
            }
        }

        let response = self
            .client
            .get(remote_url)
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        validate_image_response(&response)?;
        let bytes = response.bytes().map_err(|error| error.to_string())?;
        validate_image_bytes(&bytes, Some(image_extension(remote_url)))?;

        let temporary_path = path.with_extension(format!(
            "{}.tmp",
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or("img")
        ));
        fs::write(&temporary_path, &bytes).map_err(|error| error.to_string())?;
        if let Err(error) = validate_cached_file(&temporary_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(format!("图片写入后完整性检查失败：{error}"));
        }
        fs::rename(&temporary_path, &path).map_err(|error| error.to_string())?;
        self.remove_outdated_images(item_id, &path);
        app_log::info(format!("物品图片已写入本地缓存：{}", path.display()));
        Ok(path)
    }

    /// 根据物品 ID 和远程地址生成稳定的缓存文件名。
    fn cache_path(&self, item_id: &str, remote_url: &str) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        remote_url.hash(&mut hasher);
        self.cache_dir.join(format!(
            "{}-{:016x}.{}",
            safe_item_id(item_id),
            hasher.finish(),
            image_extension(remote_url)
        ))
    }

    /// 删除同一物品已被新地址替代的旧图片。
    fn remove_outdated_images(&self, item_id: &str, current_path: &Path) {
        let prefix = format!("{}-", safe_item_id(item_id));
        let Ok(entries) = fs::read_dir(&self.cache_dir) else {
            return;
        };

        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let belongs_to_item = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix));
            if belongs_to_item && path != current_path {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// 验证响应声明的是受支持且大小合理的图片。
fn validate_image_response(response: &reqwest::blocking::Response) -> Result<(), String> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_IMAGE_BYTES)
    {
        return Err("图片超过 5 MB 缓存限制".to_string());
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type.starts_with("image/") {
        return Err(format!("响应不是图片：{content_type}"));
    }
    Ok(())
}

/// 读取缓存文件并验证其实际格式和完整性。
fn validate_cached_file(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!("缓存文件大小不合法：{} 字节", metadata.len()));
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let expected_extension = path.extension().and_then(|value| value.to_str());
    validate_image_bytes(&bytes, expected_extension)
}

/// 识别图片格式并完整解码，拒绝伪装或截断的内容。
fn validate_image_bytes(bytes: &[u8], expected_extension: Option<&str>) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!("图片大小不合法：{} 字节", bytes.len()));
    }
    let format =
        image::guess_format(bytes).map_err(|error| format!("无法识别图片格式：{error}"))?;
    if !matches!(
        format,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
    ) {
        return Err(format!("不支持的图片格式：{format:?}"));
    }
    if let Some(expected_format) = format_for_extension(expected_extension.unwrap_or_default()) {
        if format != expected_format {
            return Err(format!(
                "图片实际格式 {format:?} 与文件扩展名 {expected_extension:?} 不一致"
            ));
        }
    }
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|error| format!("图片内容不完整：{error}"))?;
    if decoded.width() == 0 || decoded.height() == 0 {
        return Err("图片尺寸不能为零".to_string());
    }
    Ok(())
}

/// 将物品 ID 收敛为适合 Windows 文件名的字符。
fn safe_item_id(item_id: &str) -> String {
    let value: String = item_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if value.is_empty() {
        "item".to_string()
    } else {
        value
    }
}

/// 从远程地址提取受支持的图片扩展名。
fn image_extension(remote_url: &str) -> &'static str {
    let path = remote_url.split('?').next().unwrap_or(remote_url);
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        "gif" => "gif",
        _ => "img",
    }
}

/// 将缓存扩展名映射为图片解码格式。
fn format_for_extension(extension: &str) -> Option<image::ImageFormat> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "webp" => Some(image::ImageFormat::WebP),
        "gif" => Some(image::ImageFormat::Gif),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证缓存文件名会过滤 Windows 不允许的字符。
    #[test]
    fn sanitizes_item_id_for_cache_file_name() {
        assert_eq!(safe_item_id("tech/item:1"), "tech_item_1");
    }

    /// 验证常见图片地址能映射到受支持的扩展名。
    #[test]
    fn detects_supported_image_extensions() {
        assert_eq!(image_extension("https://example.test/a.png?v=1"), "png");
        assert_eq!(image_extension("https://example.test/a.jpeg"), "jpg");
        assert_eq!(image_extension("https://example.test/no-extension"), "img");
    }

    /// 验证完整图片可通过解码检查，而伪装内容会被拒绝。
    #[test]
    fn validates_image_format_and_integrity() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();

        assert!(validate_image_bytes(png.get_ref(), Some("png")).is_ok());
        assert!(validate_image_bytes(b"not an image", Some("png")).is_err());
        assert!(validate_image_bytes(png.get_ref(), Some("jpg")).is_err());
    }

    /// 验证有效缓存命中时不会尝试访问网络。
    #[test]
    fn uses_valid_cache_before_network() {
        let root = std::env::temp_dir().join(format!("df-image-cache-hit-{}", std::process::id()));
        let cache = ImageCache::new(&root);
        let remote_url = "https://cache-hit.invalid/item.png";
        let cache_path = cache.cache_path("tech", remote_url);
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        fs::write(&cache_path, png.get_ref()).unwrap();

        let resolved = cache.cache_image("tech", remote_url).unwrap();

        assert_eq!(resolved, cache_path);
        fs::remove_dir_all(root).unwrap();
    }
}
