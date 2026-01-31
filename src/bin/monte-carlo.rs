use std::{collections::HashMap};

use mini_holdem::{cards::{Card, HandCategory, get_best_hand_rank}};
use rand::{seq::SliceRandom, thread_rng};

fn main() {
    let mut results = HashMap::<HandCategory, u32>::new();

    let iters = 100000;
    for _ in 0..iters {
        let deck = get_shuffled_deck();
        let cards: &[_; 7] = &deck[0..7].try_into().unwrap();
        let category = get_best_hand_rank(cards).category;
        *results.entry(category).or_insert(0) += 1;
    }

    for (category, amount) in results {
        let probability = amount as f32 / iters as f32;
        println!("{:?} has a probability of {}%, it is 1 in {} (appeared {} times)", category, probability * 100.0, if probability > 0.0 {1.0/probability} else {69696969420.0}, amount)
    }
}

fn get_shuffled_deck() -> Vec<Card> {
    let mut deck = Vec::<Card>::new();
    for suit in 0..4 {
        for rank in 0..13 {
            deck.push(Card { rank, suit });
        }
    }

    deck.shuffle(&mut thread_rng());

    deck
}
