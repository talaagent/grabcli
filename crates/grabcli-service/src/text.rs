//! The client's answers as plain text (Markdown-ish: ids in backticks), for
//! a chat reply or a language model.

use super::api::{Cart, Menu, Merchant, Quote, Tracked, money};

fn opt<T: core::fmt::Display>(label: &str, v: Option<T>) -> String {
    v.map_or_else(String::new, |v| format!(" · {label}{v}"))
}

/// A search's merchants, one per line.
pub fn merchants(ms: &[Merchant]) -> String {
    if ms.is_empty() {
        return "No restaurants found.".into();
    }
    ms.iter()
        .map(|m| {
            let mut s = format!("- {} (`{}`)", m.name, m.id);
            if !m.cuisine.is_empty() {
                s.push_str(&format!(" · {}", m.cuisine));
            }
            s.push_str(&opt("ETA ", m.eta_min.map(|e| format!("{e} min"))));
            s.push_str(&opt("", m.distance_km.map(|d| format!("{d:.1} km"))));
            s.push_str(&opt("★", m.rating));
            if !m.promo.is_empty() {
                s.push_str(&format!(" · {}", m.promo));
            }
            if !m.open {
                s.push_str(" · closed");
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A menu, one item per line; `query`: only the items whose name, category or description has it.
pub fn menu(m: &Menu, query: Option<&str>) -> String {
    let q = query.map(str::to_lowercase).filter(|q| !q.is_empty());
    let mut out = format!("{} (`{}`)\n", m.merchant.name, m.merchant.id);
    let mut n = 0;
    for i in &m.items {
        if let Some(q) = &q
            && ![&i.name, &i.category, &i.description]
                .iter()
                .any(|f| f.to_lowercase().contains(q))
        {
            continue;
        }
        n += 1;
        out.push_str(&format!(
            "- {} (`{}`) {}",
            i.name,
            i.id,
            money(i.price, &m.currency)
        ));
        if !i.category.is_empty() {
            out.push_str(&format!(" · {}", i.category));
        }
        if !i.available {
            out.push_str(" · unavailable");
        }
        out.push('\n');
    }
    if n == 0 {
        out.push_str("No matching items.\n");
    }
    out.trim_end().to_owned()
}

/// The cart, with its subtotal.
pub fn cart(c: &Cart) -> String {
    if c.lines.is_empty() {
        return "The cart is empty.".into();
    }
    let mut out = format!("Cart at {} (`{}`)\n", c.merchant_name, c.merchant_id);
    for l in &c.lines {
        out.push_str(&format!(
            "- {}× {} (`{}`) {}",
            l.quantity,
            l.name,
            l.item_id,
            money(l.amount, &c.currency)
        ));
        if !l.note.is_empty() {
            out.push_str(&format!(" ({})", l.note));
        }
        out.push('\n');
    }
    out.push_str(&format!("Subtotal {}", money(c.subtotal, &c.currency)));
    out
}

/// A quote: lines, fees, discount, total, payment and where it goes (a
/// label). The caller adds how to order it (its own command or tool).
pub fn quote(q: &Quote) -> String {
    let c = q.currency.as_str();
    let mut out = format!("Quote `{}` at {}\n", q.quote_id, q.merchant_name);
    for l in &q.lines {
        out.push_str(&format!(
            "- {}× {} {}\n",
            l.quantity,
            l.name,
            money(l.amount, c)
        ));
    }
    out.push_str(&format!("Subtotal {}\n", money(q.subtotal, c)));
    for f in &q.fees {
        out.push_str(&format!("{} {}\n", f.label, money(f.amount, c)));
    }
    if q.discount > 0 {
        out.push_str(&format!(
            "Discount ({}) -{}\n",
            q.voucher,
            money(q.discount, c)
        ));
    }
    out.push_str(&format!("Total {}\n", money(q.total, c)));
    if let Some(e) = q.eta_min {
        out.push_str(&format!("ETA about {e} min\n"));
    }
    out.push_str(&format!(
        "Pay with {} · deliver to {}",
        q.payment_method, q.address_label
    ));
    out
}

/// Tracked orders, one per line.
pub fn tracked(ts: &[Tracked]) -> String {
    if ts.is_empty() {
        return "No active orders.".into();
    }
    ts.iter()
        .map(|t| {
            let mut s = format!("- `{}` {}: {}", t.order_id, t.merchant_name, t.status);
            s.push_str(&opt("ETA ", t.eta_min.map(|e| format!("{e} min"))));
            if !t.driver.is_empty() {
                s.push_str(&format!(" · driver {}", t.driver));
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}
