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

#[cfg(test)]
mod tests {
    use optics::HostRankings;

    use crate::{
        gen_temp_path,
        index::Index,
        ranking::inbound_similarity::InboundSimilarity,
        searcher::{LocalSearcher, SearchQuery},
        webgraph::{Node, WebgraphWriter},
        webpage::{Html, Webpage},
    };
    const CONTENT: &str = "this is the best example website ever this is the best example website ever this is the best example website ever this is the best example website ever this is the best example website ever this is the best example website ever";

    #[test]
    fn host_rankings() {
        let mut index = Index::temporary().expect("Unable to open index");

        let mut wrt = WebgraphWriter::new(
            gen_temp_path(),
            crate::executor::Executor::single_thread(),
            crate::webgraph::Compression::default(),
        );

        wrt.insert(
            Node::from("https://www.first.com").into_host(),
            Node::from("https://www.nan.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.nan.com").into_host(),
            Node::from("https://www.first.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.third.com").into_host(),
            Node::from("https://www.third.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.nan.com").into_host(),
            Node::from("https://www.second.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.second.com").into_host(),
            Node::from("https://www.nan.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.second.com").into_host(),
            Node::from("https://www.third.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.extra.com").into_host(),
            Node::from("https://www.first.com").into_host(),
            String::new(),
        );
        wrt.insert(
            Node::from("https://www.second.com").into_host(),
            Node::from("https://www.extra.com").into_host(),
            String::new(),
        );

        let graph = wrt.finalize();

        index
            .insert(Webpage {
                html: Html::parse(
                    &format!(
                        r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                {CONTENT} {}
                            </body>
                        </html>
                    "#,
                        crate::rand_words(100)
                    ),
                    "https://www.first.com",
                )
                .unwrap(),
                host_centrality: 1.0,
                fetch_time_ms: 500,
                node_id: Some(Node::from("https://www.first.com").into_host().id()),
                ..Default::default()
            })
            .expect("failed to insert webpage");
        index
            .insert(Webpage {
                html: Html::parse(
                    &format!(
                        r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                {CONTENT} {}
                            </body>
                        </html>
                    "#,
                        crate::rand_words(100)
                    ),
                    "https://www.second.com",
                )
                .unwrap(),
                host_centrality: 1.0,
                fetch_time_ms: 499,
                node_id: Some(Node::from("https://www.second.com").into_host().id()),
                ..Default::default()
            })
            .expect("failed to insert webpage");
        index
            .insert(Webpage {
                html: Html::parse(
                    &format!(
                        r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                {CONTENT} {}
                            </body>
                        </html>
                    "#,
                        crate::rand_words(100)
                    ),
                    "https://www.third.com",
                )
                .unwrap(),
                backlink_labels: vec![],
                host_centrality: 1.0,
                node_id: Some(Node::from("https://www.third.com").into_host().id()),
                ..Default::default()
            })
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let mut searcher = LocalSearcher::new(index);

        searcher.set_inbound_similarity(InboundSimilarity::build(&graph));

        let result = searcher
            .search(&SearchQuery {
                query: "test".to_string(),
                host_rankings: Some(HostRankings {
                    liked: vec!["www.first.com".to_string()],
                    disliked: vec![],
                    blocked: vec![],
                }),
                ..Default::default()
            })
            .expect("Search failed");

        assert_eq!(result.webpages.len(), 3);
        assert_eq!(result.webpages[0].url, "https://www.first.com/");
        assert_eq!(result.webpages[1].url, "https://www.second.com/");
        assert_eq!(result.webpages[2].url, "https://www.third.com/");

        let result = searcher
            .search(&SearchQuery {
                query: "test".to_string(),
                host_rankings: Some(HostRankings {
                    liked: vec!["www.first.com".to_string()],
                    disliked: vec![],
                    blocked: vec![],
                }),
                ..Default::default()
            })
            .expect("Search failed");

        assert_eq!(result.webpages.len(), 3);
        assert_eq!(result.webpages[0].url, "https://www.first.com/");
        assert_eq!(result.webpages[1].url, "https://www.second.com/");
        assert_eq!(result.webpages[2].url, "https://www.third.com/");

        let result = searcher
            .search(&SearchQuery {
                query: "test".to_string(),
                host_rankings: Some(HostRankings {
                    liked: vec![],
                    disliked: vec!["www.second.com".to_string()],
                    blocked: vec!["first.com".to_string()],
                }),
                return_ranking_signals: true,
                ..Default::default()
            })
            .expect("Search failed");

        assert_eq!(result.webpages.len(), 2);
        assert_eq!(result.webpages[0].url, "https://www.third.com/");
        assert_eq!(result.webpages[1].url, "https://www.second.com/");

        let result = searcher
            .search(&SearchQuery {
                query: "test".to_string(),
                host_rankings: Some(HostRankings {
                    liked: vec![],
                    disliked: vec![],
                    blocked: vec!["www.first.com".to_string()],
                }),
                return_ranking_signals: true,
                ..Default::default()
            })
            .expect("Search failed");

        assert_eq!(result.webpages.len(), 2);
        assert_eq!(result.webpages[0].url, "https://www.third.com/");
        assert_eq!(result.webpages[1].url, "https://www.second.com/");

        let result = searcher
            .search(&SearchQuery {
                query: "test".to_string(),
                host_rankings: Some(HostRankings {
                    liked: vec!["first.com".to_string()],
                    disliked: vec![],
                    blocked: vec!["abc.first.com".to_string()],
                }),
                return_ranking_signals: true,
                ..Default::default()
            })
            .expect("Search failed");

        assert_eq!(result.webpages.len(), 3);
        assert_eq!(result.webpages[0].url, "https://www.first.com/");
        assert_eq!(result.webpages[1].url, "https://www.second.com/");
        assert_eq!(result.webpages[2].url, "https://www.third.com/");
    }
}
