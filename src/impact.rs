use crate::model::Model;
use oxigraph::{
    model::{GraphNameRef, NamedNode, QuadRef, Term, Variable},
    sparql::{QueryResults, SparqlEvaluator},
    store::Store,
};

pub fn impact(model: &Model, iri: &str) -> Result<Vec<String>, String> {
    let start = NamedNode::new(iri).map_err(|e| e.to_string())?;
    if model.graph.triples_for_subject(&start).next().is_none() {
        return Err(format!("unknown impact IRI {iri}"));
    }
    let store = Store::new().map_err(|e| e.to_string())?;
    for t in &model.graph {
        store
            .insert(QuadRef::new(
                t.subject,
                t.predicate,
                t.object,
                GraphNameRef::DefaultGraph,
            ))
            .map_err(|e| e.to_string())?;
    }
    let query = SparqlEvaluator::new()
        .parse_query(include_str!("../queries/impact.rq"))
        .map_err(|e| e.to_string())?;
    let result = query
        .substitute_variable(Variable::new_unchecked("start"), start)
        .on_store(&store)
        .execute()
        .map_err(|e| e.to_string())?;
    let QueryResults::Solutions(rows) = result else {
        return Err("impact query must select rows".into());
    };
    rows.map(
        |row| match row.map_err(|e| e.to_string())?.get("affected") {
            Some(Term::NamedNode(n)) => Ok(n.as_str().to_owned()),
            _ => Err("impact query must select named resources".into()),
        },
    )
    .collect()
}
