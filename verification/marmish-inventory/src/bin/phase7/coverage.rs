use super::*;
/// Order retained source files by the first-party package graph. The denominator
/// counts lexical declarations once across all selected configurations.
pub fn coverage(inventory: &Value, generated: &Set, register: &Value) -> Result<Value> {
    let crates = object(&inventory["cargo"]["crates"])?;
    let sites: Vec<_> = array(&inventory["symbols"])?
        .iter()
        .filter(|s| s["classification"] == "retained-native")
        .collect();
    let mut imported = 0;
    for s in &sites {
        let path = text(&s["path"])?;
        if generated.contains(path) {
            imported += 1;
            continue;
        }
        let mut parts = path.split('/');
        ensure!(
            parts.next() == Some("crates")
                && parts.next().is_some_and(|k| crates.contains_key(k))
                && parts.next().is_some(),
            "retained path has no workspace package: {path}"
        );
    }
    let mut deps = BTreeMap::new();
    for (name, krate) in crates {
        let mut local = Set::new();
        for dep in array(&krate["dependencyDeclarations"])? {
            let name = text(&dep["name"])?;
            if crates.contains_key(name) && dep["kind"] != "dev" {
                local.insert(name.to_owned());
            }
        }
        deps.insert(name.clone(), local);
    }
    let mut order = vec![];
    while !deps.is_empty() {
        let ready: Set = deps
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        ensure!(
            !ready.is_empty(),
            "cycle among first-party package dependencies"
        );
        deps.retain(|name, _| !ready.contains(name));
        for edges in deps.values_mut() {
            edges.retain(|d| !ready.contains(d));
        }
        order.extend(ready);
    }
    let mut retained = vec![];
    for module in array(&register["modules"])? {
        let path = text(&module["path"])?;
        let krate = path.split('/').nth(1).context("missing module package")?;
        let index = order
            .iter()
            .position(|name| name == krate)
            .context("unknown residual module package")?;
        retained.push((index, path, module));
    }
    retained.sort_by_key(|(index, path, _)| (*index, *path));
    let retained: Vec<_> = retained.into_iter().map(|(_, _, m)| m).collect();
    let total = retained.iter().try_fold(0u64, |sum, m| {
        Ok::<_, anyhow::Error>(
            sum + m["lexicalSites"]
                .as_u64()
                .context("invalid lexical count")?,
        )
    })?;
    ensure!(
        total + imported == sites.len() as u64,
        "residual register does not balance the original lexical denominator"
    );
    let blocking: Vec<_> = array(&register["tasks"])?
        .iter()
        .filter(|task| retained.iter().any(|m| m["blockedOn"] == task["id"]))
        .collect();
    Ok(json!({
        "unit":"declaration sites in the frozen upstream runtime-source inventory",
        "regeneratedSites":imported,"retainedSites":sites.len() as u64-imported,"knownSites":sites.len(),
        "compiledRuntimeDenominator":null,"packageDependencyOrder":order,"retainedModules":retained,"blockingCapabilities":blocking
    }))
}
