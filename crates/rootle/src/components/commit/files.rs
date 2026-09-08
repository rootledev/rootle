//! Always-expanded, directory-first file hierarchy. Leaves retain provider
//! file indices; directory rows never participate in selection or stepping.
use super::prepare::FilePresentation;
use crate::components::list_view::ItemIndex;
use std::cmp::Ordering;
use std::ops::Range;

pub(super) enum FileTreeRow {
    Directory { label: String, depth: usize },
    File { file: ItemIndex, depth: usize },
}

struct TreeRow {
    display: FileTreeRow,
    leaves: Range<usize>,
}

pub(super) struct FileTree {
    rows: Vec<TreeRow>,
    file_order: Vec<ItemIndex>,
}

impl FileTree {
    pub fn new(files: &[FilePresentation]) -> Self {
        let mut paths: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                (
                    ItemIndex::new(index),
                    file.path.as_str().split('/').collect::<Vec<_>>(),
                )
            })
            .collect();
        paths.sort_by(|(left_index, left), (right_index, right)| {
            for index in 0..left.len().min(right.len()) {
                let directories_first = (index + 1 < right.len()).cmp(&(index + 1 < left.len()));
                let order = directories_first.then_with(|| left[index].cmp(right[index]));
                if order != Ordering::Equal {
                    return order;
                }
            }
            left.len()
                .cmp(&right.len())
                .then_with(|| left_index.get().cmp(&right_index.get()))
        });
        let mut tree = Self {
            rows: Vec::new(),
            file_order: Vec::with_capacity(files.len()),
        };
        // Iterative construction keeps arbitrarily deep provider paths off the call stack.
        let mut ancestors: Vec<(&str, usize)> = Vec::new();
        for (file, path) in paths {
            let directories = &path[..path.len() - 1];
            let common = ancestors
                .iter()
                .zip(directories)
                .take_while(|((name, _), next)| name == *next)
                .count();
            while ancestors.len() > common {
                let (_, row) = ancestors.pop().expect("nonempty ancestor stack");
                tree.rows[row].leaves.end = tree.file_order.len();
            }
            for (depth, directory) in directories.iter().enumerate().skip(common) {
                let row = tree.rows.len();
                tree.rows.push(TreeRow {
                    display: FileTreeRow::Directory {
                        label: crate::sanitize::sanitize_inline(directory),
                        depth,
                    },
                    leaves: tree.file_order.len()..tree.file_order.len(),
                });
                ancestors.push((directory, row));
            }
            let leaf = tree.file_order.len();
            tree.rows.push(TreeRow {
                display: FileTreeRow::File {
                    file,
                    depth: directories.len(),
                },
                leaves: leaf..leaf + 1,
            });
            tree.file_order.push(file);
        }
        for (_, row) in ancestors {
            tree.rows[row].leaves.end = tree.file_order.len();
        }
        tree
    }

    pub fn file_order(&self) -> &[ItemIndex] {
        &self.file_order
    }

    pub fn visible_rows(&self, visible_files: &[ItemIndex]) -> Vec<&FileTreeRow> {
        let mut included = vec![false; self.file_order.len()];
        for file in visible_files {
            if let Some(included) = included.get_mut(file.get()) {
                *included = true;
            }
        }
        let mut prefix = Vec::with_capacity(self.file_order.len() + 1);
        prefix.push(0usize);
        for file in &self.file_order {
            prefix.push(prefix.last().copied().unwrap_or(0) + usize::from(included[file.get()]));
        }
        self.rows
            .iter()
            .filter(|row| prefix[row.leaves.end] > prefix[row.leaves.start])
            .map(|row| &row.display)
            .collect()
    }
}
