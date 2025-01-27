// Stract is an open source web search engine.
// Copyright (C) 2023 Stract ApS
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::fmt::Display;
use tantivy::{
    query::{BooleanQuery, Occur, PhraseQuery, TermQuery},
    tokenizer::Tokenizer,
};

use crate::{
    bangs::BANG_PREFIXES,
    floor_char_boundary,
    schema::{Field, TextField},
};

#[derive(Debug, Clone)]
pub struct TermCompound {
    pub terms: Vec<SimpleTerm>,
}

#[derive(Debug, Clone)]
pub struct CompoundAwareTerm {
    pub term: Term,
    pub adjacent_terms: Vec<TermCompound>,
}
impl CompoundAwareTerm {
    pub fn as_tantivy_query(
        &self,
        fields: &[tantivy::schema::Field],
    ) -> (Occur, Box<dyn tantivy::query::Query + 'static>) {
        if !self.adjacent_terms.is_empty() {
            if let Term::Simple(simple_term) = &self.term {
                return simple_into_tantivy(simple_term, &self.adjacent_terms, fields);
            }
        }

        self.term.as_tantivy_query(fields)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimpleTerm(String);
impl SimpleTerm {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SimpleTerm {
    fn from(value: String) -> Self {
        SimpleTerm(value)
    }
}

impl From<SimpleTerm> for String {
    fn from(value: SimpleTerm) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Term {
    Simple(SimpleTerm),
    Phrase(String),
    Not(Box<Term>),
    Site(String),
    Title(String),
    Body(String),
    Url(String),
    PossibleBang(String),
}

impl Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Term::Simple(term) => write!(f, "{}", term.0),
            Term::Phrase(phrase) => write!(f, "\"{}\"", phrase),
            Term::Not(term) => write!(f, "-{}", term),
            Term::Site(site) => write!(f, "site:{}", site),
            Term::Title(title) => write!(f, "intitle:{}", title),
            Term::Body(body) => write!(f, "inbody:{}", body),
            Term::Url(url) => write!(f, "inurl:{}", url),
            Term::PossibleBang(bang) => write!(f, "{}{}", BANG_PREFIXES[0], bang),
        }
    }
}

fn simple_into_tantivy(
    term: &SimpleTerm,
    adjacent_terms: &[TermCompound],
    fields: &[tantivy::schema::Field],
) -> (Occur, Box<dyn tantivy::query::Query + 'static>) {
    let mut queries = Term::into_tantivy_simple(term, fields);

    let fields = fields
        .iter()
        .filter(|field| {
            matches!(
                Field::get(field.field_id() as usize),
                Some(Field::Text(TextField::AllBody))
                    | Some(Field::Text(TextField::Title))
                    | Some(Field::Text(TextField::Url))
            )
        })
        .copied()
        .collect::<Vec<_>>();

    for adjacent_term in adjacent_terms {
        let combined = adjacent_term
            .terms
            .iter()
            .map(|term| term.0.as_str())
            .collect::<String>();

        for field in &fields {
            queries.push((Occur::Should, Term::tantivy_text_query(field, &combined)))
        }
    }

    (Occur::Must, Box::new(BooleanQuery::new(queries)))
}

impl Term {
    pub fn as_simple_text(&self) -> Option<&str> {
        match self {
            Term::Simple(term) => Some(&term.0),
            Term::Phrase(p) => Some(p),
            _ => None,
        }
    }

    fn as_tantivy_query(
        &self,
        fields: &[tantivy::schema::Field],
    ) -> (Occur, Box<dyn tantivy::query::Query + 'static>) {
        match self {
            Term::Simple(term) => simple_into_tantivy(term, &[], fields),
            Term::Phrase(phrase) => {
                let mut phrases = Vec::with_capacity(fields.len());

                for (field, tv_field) in fields
                    .iter()
                    .filter_map(|tv_field| {
                        Field::get(tv_field.field_id() as usize).map(|mapped| (mapped, *tv_field))
                    })
                    .filter(|(field, _)| field.is_searchable())
                    .filter(|(field, _)| field.has_pos())
                {
                    let mut processed_terms = Term::process_tantivy_term(phrase, tv_field);

                    if processed_terms.is_empty() {
                        continue;
                    }

                    if processed_terms.len() == 1 {
                        let options = field.as_text().unwrap().index_option();

                        phrases.push((
                            Occur::Should,
                            Box::new(TermQuery::new(processed_terms.pop().unwrap(), options))
                                as Box<dyn tantivy::query::Query>,
                        ));
                    } else {
                        phrases.push((
                            Occur::Should,
                            Box::new(PhraseQuery::new(processed_terms))
                                as Box<dyn tantivy::query::Query>,
                        ));
                    }
                }

                (Occur::Must, Box::new(BooleanQuery::new(phrases)))
            }
            Term::Not(subterm) => (
                Occur::MustNot,
                Box::new(BooleanQuery::new(vec![subterm.as_tantivy_query(fields)])),
            ),
            Term::Site(site) => (
                Occur::Must,
                Box::new(BooleanQuery::new(Term::into_tantivy_site(site, fields))),
            ),
            Term::Title(title) => {
                let field = fields
                    .iter()
                    .find(|field| {
                        matches!(
                            Field::get(field.field_id() as usize),
                            Some(Field::Text(TextField::Title))
                        )
                    })
                    .unwrap();

                (Occur::Must, Term::tantivy_text_query(field, title))
            }
            Term::Body(body) => {
                let field = fields
                    .iter()
                    .find(|field| {
                        matches!(
                            Field::get(field.field_id() as usize),
                            Some(Field::Text(TextField::AllBody))
                        )
                    })
                    .unwrap();

                (Occur::Must, Term::tantivy_text_query(field, body))
            }
            Term::Url(url) => {
                let field = fields
                    .iter()
                    .find(|field| {
                        matches!(
                            Field::get(field.field_id() as usize),
                            Some(Field::Text(TextField::Url))
                        )
                    })
                    .unwrap();

                (Occur::Must, Term::tantivy_text_query(field, url))
            }
            Term::PossibleBang(text) => {
                let mut term = String::new();

                term.push(BANG_PREFIXES[0]);
                term.push_str(text);

                simple_into_tantivy(&term.into(), &[], fields)
            }
        }
    }

    fn into_tantivy_simple(
        term: &SimpleTerm,
        fields: &[tantivy::schema::Field],
    ) -> Vec<(Occur, Box<dyn tantivy::query::Query + 'static>)> {
        fields
            .iter()
            .filter_map(|tv_field| {
                Field::get(tv_field.field_id() as usize)
                    .filter(|field| field.is_searchable())
                    .map(|_| tv_field)
            })
            .map(|field| (Occur::Should, Term::tantivy_text_query(field, &term.0)))
            .collect()
    }

    fn into_tantivy_site(
        term: &str,
        fields: &[tantivy::schema::Field],
    ) -> Vec<(Occur, Box<dyn tantivy::query::Query + 'static>)> {
        fields
            .iter()
            .filter(|field| {
                matches!(
                    Field::get(field.field_id() as usize),
                    Some(Field::Text(TextField::UrlForSiteOperator))
                )
            })
            .map(|field| {
                let processed_terms = Term::process_tantivy_term(term, *field);

                if processed_terms.len() == 1 {
                    let term = processed_terms.first().unwrap().clone();
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            term,
                            tantivy::schema::IndexRecordOption::Basic,
                        )) as Box<dyn tantivy::query::Query>,
                    )
                } else {
                    (
                        Occur::Should,
                        Box::new(PhraseQuery::new(processed_terms))
                            as Box<dyn tantivy::query::Query>,
                    )
                }
            })
            .collect()
    }

    fn tantivy_text_query(
        field: &tantivy::schema::Field,
        term: &str,
    ) -> Box<dyn tantivy::query::Query + 'static> {
        let mut processed_terms = Term::process_tantivy_term(term, *field);

        let option = Field::get(field.field_id() as usize)
            .unwrap()
            .as_text()
            .unwrap()
            .index_option();

        let processed_query = if processed_terms.len() == 1 {
            let term = processed_terms.remove(0);
            Box::new(TermQuery::new(term, option)) as Box<dyn tantivy::query::Query + 'static>
        } else {
            Box::new(BooleanQuery::new(
                processed_terms
                    .into_iter()
                    .map(|term| {
                        (
                            Occur::Must,
                            Box::new(TermQuery::new(term, option))
                                as Box<dyn tantivy::query::Query + 'static>,
                        )
                    })
                    .collect(),
            )) as Box<dyn tantivy::query::Query + 'static>
        };

        Box::new(processed_query)
    }

    fn process_tantivy_term(
        term: &str,
        tantivy_field: tantivy::schema::Field,
    ) -> Vec<tantivy::Term> {
        match Field::get(tantivy_field.field_id() as usize) {
            Some(Field::Fast(_)) => vec![tantivy::Term::from_field_text(tantivy_field, term)],
            Some(Field::Text(text_field)) => {
                let mut terms: Vec<tantivy::Term> = Vec::new();
                let mut tokenizer = text_field.query_tokenizer();
                let mut token_stream = tokenizer.token_stream(term);
                token_stream.process(&mut |token| {
                    let term = tantivy::Term::from_field_text(tantivy_field, &token.text);
                    terms.push(term);
                });

                terms
            }
            None => vec![],
        }
    }
}

fn parse_term(term: &str) -> Box<Term> {
    // TODO: re-write this entire function once if-let chains become stable
    if let Some(not_term) = term.strip_prefix('-') {
        if !not_term.is_empty() && !not_term.starts_with('-') {
            Box::new(Term::Not(parse_term(not_term)))
        } else {
            Box::new(Term::Simple(term.to_string().into()))
        }
    } else if let Some(site) = term.strip_prefix("site:") {
        if !site.is_empty() {
            Box::new(Term::Site(site.to_string()))
        } else {
            Box::new(Term::Simple(term.to_string().into()))
        }
    } else if let Some(title) = term.strip_prefix("intitle:") {
        if !title.is_empty() {
            Box::new(Term::Title(title.to_string()))
        } else {
            Box::new(Term::Simple(term.to_string().into()))
        }
    } else if let Some(body) = term.strip_prefix("inbody:") {
        if !body.is_empty() {
            Box::new(Term::Body(body.to_string()))
        } else {
            Box::new(Term::Simple(term.to_string().into()))
        }
    } else if let Some(url) = term.strip_prefix("inurl:") {
        if !url.is_empty() {
            Box::new(Term::Url(url.to_string()))
        } else {
            Box::new(Term::Simple(term.to_string().into()))
        }
    } else {
        for bang_prefix in BANG_PREFIXES {
            if let Some(bang) = term.strip_prefix(bang_prefix) {
                return Box::new(Term::PossibleBang(bang.to_string()));
            }
        }

        Box::new(Term::Simple(term.to_string().into()))
    }
}

#[allow(clippy::vec_box)]
pub fn parse(query: &str) -> Vec<Box<Term>> {
    let query = query.to_lowercase().replace(['“', '”'], "\"");

    let mut res = Vec::new();

    let mut cur_term_begin = 0;

    for (offset, c) in query.char_indices() {
        if cur_term_begin > offset {
            continue;
        }

        cur_term_begin = floor_char_boundary(&query, cur_term_begin);

        if query[cur_term_begin..].starts_with('"') {
            if let Some(offset) = query[cur_term_begin + 1..].find('"') {
                let offset = offset + cur_term_begin + 1;
                res.push(Box::new(Term::Phrase(
                    query[cur_term_begin + 1..offset].to_string(),
                )));

                cur_term_begin = offset + 1;
                continue;
            }
        }
        if c.is_whitespace() {
            if offset - cur_term_begin == 0 {
                cur_term_begin = offset + 1;
                continue;
            }

            res.push(parse_term(&query[cur_term_begin..offset]));
            cur_term_begin = offset + 1;
        }
    }

    if cur_term_begin < query.len() {
        res.push(parse_term(
            &query[floor_char_boundary(&query, cur_term_begin)..query.len()],
        ));
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_not() {
        assert_eq!(
            parse("this -that"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Not(Box::new(Term::Simple("that".to_string().into()))))
            ]
        );

        assert_eq!(
            parse("this -"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Simple("-".to_string().into()))
            ]
        );
    }

    #[test]
    fn double_not() {
        assert_eq!(
            parse("this --that"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Simple("--that".to_string().into()))
            ]
        );
    }

    #[test]
    fn site() {
        assert_eq!(
            parse("this site:test.com"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Site("test.com".to_string()))
            ]
        );
    }

    #[test]
    fn title() {
        assert_eq!(
            parse("this intitle:test"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Title("test".to_string()))
            ]
        );
    }

    #[test]
    fn body() {
        assert_eq!(
            parse("this inbody:test"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Body("test".to_string()))
            ]
        );
    }

    #[test]
    fn url() {
        assert_eq!(
            parse("this inurl:test"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Url("test".to_string()))
            ]
        );
    }

    #[test]
    fn empty() {
        assert_eq!(parse(""), vec![]);
    }

    #[test]
    fn phrase() {
        assert_eq!(
            parse("\"this is a\" inurl:test"),
            vec![
                Box::new(Term::Phrase("this is a".to_string(),)),
                Box::new(Term::Url("test".to_string()))
            ]
        );
        assert_eq!(
            parse("\"this is a inurl:test"),
            vec![
                Box::new(Term::Simple("\"this".to_string().into())),
                Box::new(Term::Simple("is".to_string().into())),
                Box::new(Term::Simple("a".to_string().into())),
                Box::new(Term::Url("test".to_string()))
            ]
        );
        assert_eq!(
            parse("this is a\" inurl:test"),
            vec![
                Box::new(Term::Simple("this".to_string().into())),
                Box::new(Term::Simple("is".to_string().into())),
                Box::new(Term::Simple("a\"".to_string().into())),
                Box::new(Term::Url("test".to_string()))
            ]
        );

        assert_eq!(
            parse("\"this is a inurl:test\""),
            vec![Box::new(Term::Phrase("this is a inurl:test".to_string(),)),]
        );

        assert_eq!(
            parse("\"\""),
            vec![Box::new(Term::Phrase("".to_string(),)),]
        );
        assert_eq!(
            parse("“this is a“ inurl:test"),
            vec![
                Box::new(Term::Phrase("this is a".to_string(),)),
                Box::new(Term::Url("test".to_string()))
            ]
        );
    }

    #[test]
    fn unicode() {
        let query = "\u{a0}";
        assert_eq!(parse(query).len(), 1);
    }

    proptest! {
        #[test]
        fn prop(query: String) {
            parse(&query);
        }
    }
}
