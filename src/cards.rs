use std::{cmp::Ordering, collections::HashMap};

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

impl Card {
    pub fn to_byte(&self) -> u8 {
        // 00ssrrrr
        self.suit << 4 | self.rank
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
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
    pub fn to_byte(&self) -> u8 {
        match *self {
            HandCategory::HighCard => 0u8,
            HandCategory::OnePair => 1u8,
            HandCategory::TwoPair => 2u8,
            HandCategory::ThreeKind => 3u8,
            HandCategory::Straight => 4u8,
            HandCategory::Flush => 5u8,
            HandCategory::FullHouse => 6u8,
            HandCategory::FourKind => 7u8,
            HandCategory::StraightFlush => 8u8,
            HandCategory::RoyalFlush => 9u8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandRank {
    pub category: HandCategory,
    pub primary: Option<Card>,
    pub secondary: Option<Card>,
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

pub enum ShowdownDecidingFactor {
    Category,
    Primary(Card, Card),
    Secondary(Card, Card),
    Kicker(Card, Card),
    None,
}

fn get_all_combinations(cards: &[Card; 7]) -> [[Card; 5]; 21] {
    let mut returned = Vec::with_capacity(21);
    for mask in 0u32..(1u32 << 7) {
        if mask.count_ones() == 5 {
            let mut combo = Vec::with_capacity(5);
            for (i, card) in cards.iter().enumerate() {
                if (mask >> i) & 1 == 1 {
                    combo.push(*card);
                }
            }
            returned.push(combo.try_into().unwrap());
        }
    }
    returned.try_into().unwrap()
}

fn rank_hand(cards: &[Card; 5]) -> HandRank {
    let mut hand = *cards;
    hand.sort_by(|a, b| a.rank.cmp(&b.rank));

    let is_flush = hand.into_iter().map(|c| c.suit).all(|c| c == hand[0].suit);

    let is_low_ace = hand[0].rank == 0 && hand[1].rank == 1 && hand[2].rank == 2 && hand[3].rank == 3 && hand[4].rank == 12;
    let is_straight = is_low_ace || hand.windows(2).all(|w| w[0].rank + 1 == w[1].rank);

    let mut rank_counts = HashMap::new();
    for card in &hand {
        *rank_counts.entry(card.rank).or_insert(0) += 1;
    }
    let mut counts: Vec<_> = rank_counts.values().cloned().collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));

    let mut groups: Vec<(u8, usize, Vec<Card>)> = rank_counts
        .iter()
        .map(|(&rank, &count)| {
            let cards = hand.iter().cloned().filter(|c| c.rank == rank).collect();
            (rank, count, cards)
        })
        .collect();
    
    groups.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(b.0.cmp(&a.0))
    });

    // primary and secondary are only filled if they are in a group larger than one card, the rest goes to kickers
    let mut primary = None;
    let mut secondary = None;
    let mut kickers = Vec::<Card>::new();
    for (i, group) in groups.iter().enumerate() {
        if i == 0 && group.1 > 1 {
            primary = group.2.first().copied();
        } else if i == 1 && group.1 > 1 {
            secondary = group.2.first().copied();
        } else {
            kickers.push(*group.2.first().unwrap());
        }
    }

    let category = match (&counts[..], is_straight, is_flush) {
        ([1, 1, 1, 1, 1], true, true) => {
            if hand[0].rank == 8 {
                HandCategory::RoyalFlush
            } else {
                HandCategory::StraightFlush
            }
        },
        ([4, 1], _, _) => HandCategory::FourKind,
        ([3, 2], _, _) => HandCategory::FullHouse,
        ([3, 1, 1], _, _) => HandCategory::ThreeKind,
        ([2, 2, 1], _, _) => HandCategory::TwoPair,
        ([2, 1, 1, 1], _, _) => HandCategory::OnePair,
        ([1, 1, 1, 1, 1], false, true) => HandCategory::Flush,
        ([1, 1, 1, 1, 1], true, false) => HandCategory::Straight,
        _ => HandCategory::HighCard
    };

    HandRank { category, primary, secondary, kickers }
}

pub fn get_best_hand_rank(cards: &[Card; 7]) -> HandRank {
    get_all_combinations(cards).map(|c| rank_hand(&c)).iter().max().unwrap().clone()
}

pub fn compare_hand_ranks(hand1: &HandRank, hand2: &HandRank) -> (Ordering, ShowdownDecidingFactor) { // the two cards are the deciding factors in the comparison
    let category_comparison = hand1.category.cmp(&hand2.category);
    if category_comparison != Ordering::Equal {
        return (category_comparison, ShowdownDecidingFactor::Category);
    }

    if let Some(a) = hand1.primary && let Some(b) = hand2.primary {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Primary(a, b));
        }
    }

    if let Some(a) = hand1.secondary && let Some(b) = hand2.secondary {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Secondary(a, b));
        }
    }

    // both hand1.kickers and hand2.kickers are guaranteed to have the same amount of kickers
    for (&a, &b) in hand1.kickers.iter().zip(hand2.kickers.iter()) {
        let comparison = a.cmp(&b);
        if comparison != Ordering::Equal {
            return (comparison, ShowdownDecidingFactor::Kicker(a, b));
        }
    }

    (Ordering::Equal, ShowdownDecidingFactor::None)
}
