use std::{cmp::Ordering, fmt::{Display, Error}};

#[derive(Debug, Clone, Copy)]
pub struct Card {
    pub rank: u8, // 0 to 8 is 2 to 10, then 9 - J, 10 - Q, 11 - K, 12 - A
    pub suit: u8, // who cares which is which until we make them display
}
impl Ord for Card {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank.cmp(&other.rank)
    }
}
impl PartialOrd for Card {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Card {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank
    }
}
impl Eq for Card {}
impl Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}\x1b[0m",
            match self.rank {
                0..9 => (self.rank+2).to_string(),
                9 => String::from("J"),
                10 => String::from("Q"),
                11 => String::from("K"),
                12 => String::from("A"),
                _ => return Err(Error)
            },
            match self.suit {
                0 => "\x1b[31m♥",
                1 => "\x1b[31m♦",
                2 => "\x1b[30m♠",
                3 => "\x1b[30m♣",
                _ => return Err(Error)
            }
        )
    }
}

impl Card {
    pub fn to_byte(&self) -> u8 {
        // 00ssrrrr
        self.suit << 4 | self.rank
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        let rank = byte & 0x0F;
        if rank > 12 {
            return None;
        }
        Some(Card { rank, suit: byte >> 4 })
    }
}

pub fn format_cards(cards: &[Card]) -> String {
    cards.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" ")
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
#[repr(u8)]
pub enum HandCategory {
    HighCard,
    OnePair,
    TwoPair,
    ThreeKind,
    Straight,
    Flush,
    FullHouse,
    FourKind,
    StraightFlush,
    RoyalFlush,
}
impl HandCategory {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => HandCategory::HighCard,
            1 => HandCategory::OnePair,
            2 => HandCategory::TwoPair,
            3 => HandCategory::ThreeKind,
            4 => HandCategory::Straight,
            5 => HandCategory::Flush,
            6 => HandCategory::FullHouse,
            7 => HandCategory::FourKind,
            8 => HandCategory::StraightFlush,
            9 => HandCategory::RoyalFlush,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HandRank {
    pub category: HandCategory,
    pub primary: Vec<Card>,
    pub secondary: Vec<Card>,
    pub kickers: Vec<Card>,
}
impl Ord for HandRank {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_hand_ranks(self, other).0
    }
}
impl PartialOrd for HandRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for HandRank {
    fn eq(&self, other: &Self) -> bool {
        self.category == other.category
            && self.primary == other.primary
            && self.secondary == other.secondary
            && self.kickers == other.kickers
    }
}
impl Eq for HandRank {}
impl Display for HandRank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.category {
            HandCategory::HighCard => write!(f, "High card with kickers {}", format_cards(&self.kickers)),
            HandCategory::OnePair => write!(f, "Pair with cards {} and kickers {}", format_cards(&self.primary), format_cards(&self.kickers)),
            HandCategory::TwoPair => write!(f, "Two pairs {} and {} with kicker {}", format_cards(&self.primary), format_cards(&self.secondary), self.kickers[0]),
            HandCategory::ThreeKind => write!(f, "Three of a kind with cards {} with kickers {}", format_cards(&self.primary), format_cards(&self.kickers)),
            HandCategory::Straight => write!(f, "Straight with cards {}", format_cards(&self.kickers)),
            HandCategory::Flush => write!(f, "Flush with cards {}", format_cards(&self.kickers)),
            HandCategory::FullHouse => write!(f, "Full house with card triple {} and pair {}", format_cards(&self.primary), format_cards(&self.secondary)),
            HandCategory::FourKind => write!(f, "Four of a kind with cards {} and kicker {}", format_cards(&self.primary), self.kickers[0]),
            HandCategory::StraightFlush => write!(f, "Straight flush with cards {}", format_cards(&self.kickers)),
            HandCategory::RoyalFlush => write!(f, "Royal flush with cards {}", format_cards(&self.kickers))
        }
    }
}

#[derive(Debug, Clone)]
pub enum ShowdownDecidingFactor {
    Category,
    Primary(Vec<Card>, Vec<Card>),
    Secondary(Vec<Card>, Vec<Card>),
    Kicker(Vec<Card>, Vec<Card>),
    Tie,
}

fn get_all_combinations(cards: &[Card; 7]) -> [[Card; 5]; 21] {
    let mut out = [[cards[0]; 5]; 21];
    let mut n = 0;

    for a in 0..3 {
        for b in (a + 1)..4 {
            for c in (b + 1)..5 {
                for d in (c + 1)..6 {
                    for e in (d + 1)..7 {
                        out[n] = [
                            cards[a],
                            cards[b],
                            cards[c],
                            cards[d],
                            cards[e],
                        ];
                        n += 1;
                    }
                }
            }
        }
    }

    out
}

fn rank_hand(cards: &[Card; 5]) -> HandRank {
    let mut hand = *cards;
    hand.sort_by(|a, b| a.rank.cmp(&b.rank));

    let is_flush = hand.into_iter().map(|c| c.suit).all(|c| c == hand[0].suit);

    let is_low_ace = hand[0].rank == 0 && hand[1].rank == 1 && hand[2].rank == 2 && hand[3].rank == 3 && hand[4].rank == 12;
    let is_straight = is_low_ace || hand.windows(2).all(|w| w[0].rank + 1 == w[1].rank);

    let mut groups: [Vec<Card>; 13] = Default::default();
    for card in &hand {
        groups[card.rank as usize].push(*card);
    }

    groups.sort_by(|a, b| {
        b.len().cmp(&a.len())
    });

    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut kickers = Vec::<Card>::new();
    for (i, group) in groups.iter().enumerate() {
        if i == 0 && group.len() > 1 {
            primary = group.clone();
        } else if i == 1 && group.len() > 1 {
            secondary = group.clone();
        } else if !group.is_empty() {
            kickers.push(*group.first().unwrap());
        }
    }

    kickers.sort_by(|a, b| b.cmp(a));

    if primary.len() == secondary.len() && let Some(primary_card) = primary.first() && let Some(secondary_card) = secondary.first() && secondary_card.rank > primary_card.rank {
        let temp = primary;
        primary = secondary;
        secondary = temp;
    }

    let counts = [groups[0].len(), groups[1].len(), groups[2].len(), groups[3].len(), groups[4].len()];
    let category = match (counts, is_straight, is_flush) {
        ([1, 1, 1, 1, 1], true, true) => {
            if hand[0].rank == 8 {
                HandCategory::RoyalFlush
            } else {
                HandCategory::StraightFlush
            }
        },
        ([4, 1, 0, 0, 0], _, _) => HandCategory::FourKind,
        ([3, 2, 0, 0, 0], _, _) => HandCategory::FullHouse,
        ([3, 1, 1, 0, 0], _, _) => HandCategory::ThreeKind,
        ([2, 2, 1, 0, 0], _, _) => HandCategory::TwoPair,
        ([2, 1, 1, 1, 0], _, _) => HandCategory::OnePair,
        ([1, 1, 1, 1, 1], false, true) => HandCategory::Flush,
        ([1, 1, 1, 1, 1], true, false) => HandCategory::Straight,
        _ => HandCategory::HighCard
    };

    HandRank { category, primary, secondary, kickers }
}

pub fn get_best_hand_rank(cards: &[Card; 7]) -> ([Card; 5], HandRank) {
    let mut hand_ranks = get_all_combinations(cards).map(|c| (c, rank_hand(&c)));
    hand_ranks.sort_by(|a, b| b.1.cmp(&a.1));
    hand_ranks[0].clone()
}

pub fn compare_hand_ranks(hand1: &HandRank, hand2: &HandRank) -> (Ordering, ShowdownDecidingFactor) {
    let category_comparison = hand1.category.cmp(&hand2.category);
    if category_comparison != Ordering::Equal {
        return (category_comparison, ShowdownDecidingFactor::Category);
    }

    if let Some(a) = hand1.primary.first() && let Some(b) = hand2.primary.first() {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Primary(hand1.primary.clone(), hand2.primary.clone()));
        }
    }

    if let Some(a) = hand1.secondary.first() && let Some(b) = hand2.secondary.first() {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Secondary(hand1.secondary.clone(), hand2.secondary.clone()));
        }
    }

    // both hand1.kickers and hand2.kickers are guaranteed to have the same amount of kickers
    for (&a, &b) in hand1.kickers.iter().zip(hand2.kickers.iter()) {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Kicker(hand1.kickers.clone(), hand2.kickers.clone()));
        }
    }

    (Ordering::Equal, ShowdownDecidingFactor::Tie)
}
