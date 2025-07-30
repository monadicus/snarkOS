// Copyright (c) 2019-2025 Provable Inc.
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use snarkos_node::{Node, router::Peer};
use snarkvm::prelude::Network;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Span, Text},
    widgets::{Block, Borders, Paragraph, Row, Table, canvas::Canvas},
};

pub(crate) struct Overview;

impl Overview {
    fn draw_latest_block<N: Network>(&self, f: &mut Frame, area: Rect, node: &Node<N>) {
        let text = if let Some(ledger) = node.ledger() {
            let block = ledger.latest_block();
            Text::raw(format!("Hash: {} | Height: {}", block.hash(), block.height()))
        } else {
            Text::raw("N/A")
        };

        let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Latest Block"));
        f.render_widget(&paragraph, area);
    }

    /// Draws a table containing all connected and connecting peers.
    fn draw_peer_table<N: Network>(&self, f: &mut Frame, area: Rect, node: &Node<N>) {
        let header = ["IP", "State", "Node Type"];
        let constraints = [Constraint::Length(20), Constraint::Length(10), Constraint::Length(10)];

        let rows: Vec<_> = node
            .router()
            .get_peers()
            .into_iter()
            .filter(|peer| !peer.is_candidate()) // Too many candidate peers for overview.
            .map(|peer| {
                let state = if peer.is_candidate() {
                    "candidate"
                } else if peer.is_connecting() {
                    "connecting"
                } else if peer.is_connected() {
                    "connected"
                } else {
                    unreachable!("Unknown peer state");
                }.to_string();

                let node_type = if let Some(node_type ) = peer.node_type() {
                    node_type.to_string()
                } else {
                    "unknown".to_string()
                };

                let last_seen = if let Peer::Connected(p) = &peer {
                    format!("{:.2}s ago", p.last_seen.elapsed().as_secs_f64())
                } else {
                    "N/A".to_string()
                };

                Row::new([format!("{:?}", peer.listener_addr()), state, node_type, last_seen])
            })
            .collect();

        let peer_table = Table::new(rows, constraints)
            .header(Row::new(header))
            .block(Block::default().borders(Borders::ALL).title("Peers"));

        f.render_widget(peer_table, area);
    }

    pub(crate) fn draw<N: Network>(&self, f: &mut Frame, area: Rect, node: &Node<N>) {
        // Initialize the layout of the page.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)]) // The box border adds a line on both sides.
            .split(area);

        self.draw_latest_block(f, chunks[0], node);
        self.draw_peer_table(f, chunks[1], node);

        let canvas = Canvas::default().block(Block::default().borders(Borders::ALL).title("Help")).paint(|ctx| {
            ctx.print(0f64, 0f64, Span::styled("Press ESC to quit", Style::default().fg(Color::White)));
        });
        f.render_widget(canvas, chunks[2]);
    }
}
