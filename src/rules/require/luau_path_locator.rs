use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use super::{path_iterator, LuauRequireMode};
use crate::rules::require::path_utils::{get_relative_parent_path, is_require_relative};
use crate::{utils, DarkluaError, Resources};

/// A path locator specifically for Luau require mode that implements
/// the behavior defined in the Luau RFCs for module path resolution.
#[derive(Clone, Debug)]
pub(crate) struct LuauPathLocator<'b, 'resources> {
    luau_require_mode: RefCell<LuauRequireMode>,
    extra_module_relative_location: &'b Path,
    resources: &'resources Resources,
}

impl<'b, 'c> LuauPathLocator<'b, 'c> {
    pub(crate) fn new(
        luau_require_mode: LuauRequireMode,
        extra_module_relative_location: &'b Path,
        resources: &'c Resources,
    ) -> Self {
        Self {
            luau_require_mode: RefCell::new(luau_require_mode),
            extra_module_relative_location,
            resources,
        }
    }

    fn load_aliases(&self, source: &Path) -> Result<(), DarkluaError> {
        self.luau_require_mode
            .borrow_mut()
            .load_aliases(source, self.resources)
    }
}

impl super::PathLocator for LuauPathLocator<'_, '_> {
    fn find_require_path(
        &self,
        path: impl Into<PathBuf>,
        source: &Path,
    ) -> Result<PathBuf, DarkluaError> {
        let mut path = path.into();
        let literal_path = path.display().to_string();
        log::trace!(
            "find require path for `{}` from `{}` (luau mode)",
            path.display(),
            source.display()
        );
        self.load_aliases(source)?;

        let require_mode = self.luau_require_mode.borrow();
        // the alias and its source location, when the path goes through one
        let mut alias_source: Option<(String, PathBuf)> = None;

        if is_require_relative(&path) {
            if require_mode.is_module_folder_name(source) {
                path = get_relative_parent_path(get_relative_parent_path(source)).join(path);
            } else {
                path = get_relative_parent_path(source).join(path);
            }
        } else if !path.is_absolute() {
            let mut components = path.components();
            let root = components.next().ok_or_else(|| {
                DarkluaError::invalid_resource_path(path.display().to_string(), "path is empty")
            })?;
            let source_name = utils::convert_os_string(root.as_os_str()).map_err(|err| {
                err.context(format!(
                    "cannot convert source name to utf-8 in `{}`",
                    path.display(),
                ))
            })?;

            if source_name == "@self" {
                path = get_relative_parent_path(source).join(components);
            } else if source_name.starts_with("@") {
                let mut extra_module_location = require_mode
                    .get_source(source_name, self.extra_module_relative_location)
                    .ok_or_else(|| {
                        DarkluaError::unresolved_alias(
                            path.display().to_string(),
                            source_name,
                            None,
                        )
                    })?;
                alias_source = Some((source_name.to_owned(), extra_module_location.clone()));
                extra_module_location.extend(components);
                path = extra_module_location;
            }
        }

        let normalized_path = utils::normalize_path_with_current_dir(&path);
        for potential_path in
            path_iterator::find_require_paths(&normalized_path, require_mode.module_folder_name())
        {
            if self.resources.is_file(&potential_path)? {
                return Ok(utils::normalize_path_with_current_dir(potential_path));
            }
        }

        if let Some((alias, source_location)) = alias_source {
            if !path_iterator::source_location_exists(
                self.resources,
                &source_location,
                require_mode.module_folder_name(),
            )? {
                return Err(DarkluaError::unresolved_alias(
                    literal_path,
                    alias,
                    Some(source_location),
                ));
            }
        }

        Err(
            DarkluaError::resource_not_found(&normalized_path).context(format!(
                "tried `{}`",
                path_iterator::find_require_paths(
                    &normalized_path,
                    require_mode.module_folder_name(),
                )
                .map(|potential_path| potential_path.display().to_string())
                .collect::<Vec<_>>()
                .join("`, `")
            )),
        )
    }
}
