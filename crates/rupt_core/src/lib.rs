pub mod config;
pub mod conftest;
pub mod coverage;
pub mod discovery;
pub mod executor;
pub mod fixtures;
pub mod junit;
pub mod markers;
pub mod parametrize;
pub mod parser;
pub mod reporting;
pub mod results;
pub mod selection;

use config::PytestConfig;
use conftest::ConftestMap;
use discovery::discover_test_files;
use parser::TestItem;
use rayon::prelude::*;
use selection::filter_items;
use std::path::Path;

pub struct CollectionResult {
    pub items: Vec<TestItem>,
    pub conftest_map: ConftestMap,
}

pub fn collect(root: &Path, config: &PytestConfig, k_expr: Option<&str>, m_expr: Option<&str>) -> CollectionResult {
    let files = discover_test_files(root, config);

    let conftest_map = conftest::build_conftest_map(root, config);

    let raw_items: Vec<TestItem> = files
        .par_iter()
        .flat_map(|path| {
            let relative = path.strip_prefix(root).unwrap_or(path);
            parser::parse_test_file(path, relative)
        })
        .collect();

    // Expand parametrized tests into individual node IDs
    let mut items = Vec::new();
    for item in raw_items {
        if item.parametrize.is_empty() {
            items.push(item);
        } else {
            let expanded_ids =
                parametrize::expand_parametrize(&item.node_id, &item.parametrize);
            for expanded_id in expanded_ids {
                items.push(TestItem {
                    node_id: expanded_id,
                    parametrize: item.parametrize.clone(),
                    markers: item.markers.clone(),
                    is_method: item.is_method,
                    class_name: item.class_name.clone(),
                    function_name: item.function_name.clone(),
                });
            }
        }
    }

    let items = filter_items(items, k_expr, m_expr, config);

    CollectionResult {
        items,
        conftest_map,
    }
}
