use crate::languages::java::business::model::{
    CandidateScore, CandidateSignal, CodeModel, ContextPacket, EvidenceRange,
};

pub fn score_candidates(model: &mut CodeModel) {
    let mut scores = Vec::new();
    let mut signals = Vec::new();
    let mut evidence = Vec::new();
    let mut contexts = Vec::new();

    for method in &model.methods {
        let method_signals = method_signals(&method.body_text, &method.annotations);
        let score = method_signals
            .iter()
            .map(|(_, count, weight)| count * weight)
            .sum::<i64>();
        let priority = if score >= 18 {
            "high"
        } else if score >= 8 {
            "medium"
        } else {
            "low"
        };

        scores.push(CandidateScore {
            method_id: method.id.clone(),
            score,
            priority: priority.to_string(),
        });
        for (name, count, weight) in method_signals {
            if count > 0 {
                signals.push(CandidateSignal {
                    method_id: method.id.clone(),
                    name,
                    count,
                    weight,
                });
            }
        }
        evidence.push(EvidenceRange {
            method_id: method.id.clone(),
            file: method.file.clone(),
            start_line: method.start_line,
            end_line: method.end_line,
            source: "tree_sitter".to_string(),
        });
        contexts.push(ContextPacket {
            method_id: method.id.clone(),
            summary: format!(
                "{}:{}-{} score={} priority={}",
                method.file, method.start_line, method.end_line, score, priority
            ),
        });
    }

    model.candidate_scores = scores;
    model.candidate_signals = signals;
    model.evidence_ranges = evidence;
    model.context_packets = contexts;
}

fn method_signals(body: &str, annotations: &[String]) -> Vec<(String, i64, i64)> {
    let lower = body.to_ascii_lowercase();
    vec![
        (
            "branches".to_string(),
            count_terms(body, &["if", "else if", "?"]),
            3,
        ),
        (
            "switches".to_string(),
            count_terms(body, &["switch", "case "]),
            3,
        ),
        (
            "loops".to_string(),
            count_terms(body, &["for", "while", "do {"]),
            2,
        ),
        (
            "exceptions".to_string(),
            count_terms(body, &["throw ", "catch "]),
            3,
        ),
        (
            "assignments".to_string(),
            body.matches('=').count() as i64,
            1,
        ),
        (
            "method_calls".to_string(),
            body.matches('(').count() as i64,
            1,
        ),
        (
            "persistence_terms".to_string(),
            count_terms(
                &lower,
                &[
                    "repository",
                    ".save(",
                    ".delete(",
                    ".update(",
                    "entitymanager",
                    "@transactional",
                ],
            ),
            3,
        ),
        (
            "business_terms".to_string(),
            count_terms(
                &lower,
                &[
                    "approve", "reject", "validate", "status", "order", "customer", "payment",
                    "invoice", "policy", "rule",
                ],
            ),
            2,
        ),
        (
            "framework_annotations".to_string(),
            annotations
                .iter()
                .filter(|annotation| {
                    let lower = annotation.to_ascii_lowercase();
                    lower.contains("transactional")
                        || lower.contains("preauthorize")
                        || lower.contains("postauthorize")
                        || lower.contains("mapping")
                        || lower.contains("listener")
                        || lower.contains("scheduled")
                })
                .count() as i64,
            2,
        ),
    ]
}

fn count_terms(value: &str, terms: &[&str]) -> i64 {
    terms
        .iter()
        .map(|term| value.matches(term).count() as i64)
        .sum()
}

#[cfg(test)]
mod tests {
    use crate::languages::java::business::model::{CodeModel, MethodInfo};

    use super::*;

    #[test]
    fn scores_business_heavy_method_as_high_priority() {
        let mut model = CodeModel {
            methods: vec![MethodInfo {
                id: "method:demo.OrderService#approve(Long)@1".to_string(),
                module_id: "module:.".to_string(),
                class_id: "class:demo.OrderService".to_string(),
                name: "approve".to_string(),
                signature: "approve(Long)".to_string(),
                return_type: Some("void".to_string()),
                parameters: Vec::new(),
                annotations: vec!["@Transactional".to_string()],
                file: "OrderService.java".to_string(),
                start_line: 1,
                end_line: 12,
                name_line: 2,
                name_column: 7,
                body_text: r#"
                {
                  if (order.status != PENDING) throw new InvalidOrderException();
                  order.setStatus(APPROVED);
                  repository.save(order);
                }
                "#
                .to_string(),
            }],
            ..CodeModel::default()
        };

        score_candidates(&mut model);

        assert_eq!(model.candidate_scores[0].priority, "high");
        assert!(
            model
                .candidate_signals
                .iter()
                .any(|signal| signal.name == "persistence_terms")
        );
    }
}
