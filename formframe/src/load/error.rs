use {serde_yaml::Error as YamlError, thiserror::Error};

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("Duplicate root node name: {}, each root must have a unique name", .0)]
    DuplicateRootName(String),
    #[error("Failed to deserialize yaml: {}", .source)]
    YamlError {
        #[from]
        source: YamlError,
    },
}
