#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

"""Generate context decay model diagrams for docs/hooks-and-ways/context-decay.md"""

import numpy as np
import matplotlib.pyplot as plt
import matplotlib.font_manager as fm
from pathlib import Path

OUTPUT_DIR = Path(__file__).parent

# --- Style ---
# Clean, modern style that reads well on both light and dark GitHub backgrounds.
# Use a subtle off-white background so the plot area is visible on dark themes
# without being harsh on light themes.

plt.rcParams.update({
    'font.family': 'sans-serif',
    'font.sans-serif': ['DejaVu Sans', 'Helvetica', 'Arial'],
    'font.size': 11,
    'axes.linewidth': 0.8,
    'axes.edgecolor': '#4A5568',
    'axes.labelcolor': '#2D3748',
    'axes.titlesize': 14,
    'axes.titleweight': 'bold',
    'xtick.color': '#4A5568',
    'ytick.color': '#4A5568',
    'grid.color': '#CBD5E0',
    'grid.linewidth': 0.5,
    'grid.alpha': 0.7,
    'figure.facecolor': '#F7FAFC',
    'axes.facecolor': '#FFFFFF',
    'text.color': '#2D3748',
    'legend.framealpha': 0.9,
    'legend.edgecolor': '#CBD5E0',
})

# Color palette from the project's Mermaid style guide
TEAL = '#2D7D9A'
PURPLE = '#7B2D8E'
GREEN = '#2D8E5E'
ORANGE = '#C2572A'
SLATE = '#5A6ABF'
AMBER = '#8E6B2D'

# --------------------------------------------------------------------------
# Helper: generate damped sawtooth
# --------------------------------------------------------------------------

def damped_sawtooth(t, user_turns, alpha=0.35, beta=0.8, A0=1.0):
    """
    Model adherence as A0 * n^-alpha * exp(-beta * t_local).
    user_turns: list of t values where user messages occur (partial reset).
    Each user turn resets t_local to 0; the peak at reset equals the envelope.
    Returns adherence array same shape as t.
    """
    adherence = np.zeros_like(t)
    turn_idx = 0
    n = 1  # turn counter

    for i, ti in enumerate(t):
        # Check if we've passed a user turn boundary
        while turn_idx < len(user_turns) and ti >= user_turns[turn_idx]:
            n += 1
            turn_idx += 1

        # t_local: tokens since last user message
        if turn_idx > 0:
            t_local = ti - user_turns[turn_idx - 1]
        else:
            t_local = ti

        envelope = A0 * (n ** -alpha)
        local_decay = np.exp(-beta * t_local)
        adherence[i] = envelope * local_decay

    return np.clip(adherence, 0, A0)


def injected_adherence(t, user_turns, way_injections, alpha=0.35, beta=0.8,
                       A0=1.0, A_inject=0.7):
    """
    Combined adherence: decaying system prompt + fresh injections.
    way_injections: list of t values where ways fire.
    The injection term uses exp(-beta * t_since_inject) — same local decay,
    but no turn-count envelope because it's not pinned at position zero.
    """
    base = damped_sawtooth(t, user_turns, alpha, beta, A0)

    inject_component = np.zeros_like(t)
    for inj_t in way_injections:
        for i, ti in enumerate(t):
            if ti >= inj_t:
                t_since = ti - inj_t
                contrib = A_inject * np.exp(-beta * t_since)
                inject_component[i] = max(inject_component[i], contrib)

    combined = base + inject_component
    return np.clip(combined, 0, 1.3)


# --------------------------------------------------------------------------
# Figure 1: Damped Sawtooth (no ways)
# --------------------------------------------------------------------------

def fig_damped_sawtooth():
    fig, ax = plt.subplots(figsize=(10, 4.5))

    t = np.linspace(0.1, 30, 2000)
    user_turns = [5, 10, 15, 20, 25]

    alpha, beta = 0.38, 0.55
    adherence = damped_sawtooth(t, user_turns, alpha=alpha, beta=beta)

    # Fill under the curve
    ax.fill_between(t, adherence, alpha=0.15, color=ORANGE)
    ax.plot(t, adherence, color=ORANGE, linewidth=2, label='System prompt adherence')

    # Draw the decaying envelope (peak at each turn = A0 * n^-alpha)
    envelope_t = np.array([0.1] + user_turns)
    envelope_peaks = []
    for n_val, ut in enumerate(envelope_t, 1):
        peak = 1.0 * (n_val ** -alpha)
        envelope_peaks.append(peak)
    # Extend envelope to end
    envelope_t_ext = np.append(envelope_t, 30)
    envelope_peaks.append(1.0 * ((len(envelope_t) + 1) ** -alpha))

    ax.plot(envelope_t_ext, envelope_peaks, '--', color=PURPLE, linewidth=1.5,
            alpha=0.7, label=r'Peak envelope ($n^{-\alpha}$)')

    # Mark user turns
    for ut in user_turns:
        ax.axvline(ut, color=SLATE, linewidth=0.6, alpha=0.3, linestyle=':')

    # Noise floor
    ax.axhline(0.15, color='#A0AEC0', linewidth=1, linestyle='--', alpha=0.6)
    ax.text(28.5, 0.17, 'noise floor', ha='right', fontsize=9, color='#A0AEC0',
            style='italic')

    # Annotations
    ax.annotate('user messages\npartially reset\nlocal attention', xy=(10, 0.58),
                fontsize=8.5, color=SLATE, ha='center',
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=SLATE, alpha=0.8))

    ax.set_xlabel('Conversation progression (tokens / turns)', fontsize=11)
    ax.set_ylabel('Effective adherence', fontsize=11)
    ax.set_title('System Prompt Adherence: The Damped Sawtooth', pad=12)
    ax.set_xlim(0, 30)
    ax.set_ylim(0, 1.1)
    ax.set_xticks([])
    ax.set_yticks([0, 0.25, 0.5, 0.75, 1.0])
    ax.legend(loc='upper right', fontsize=9)
    ax.grid(True, axis='y')

    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'context-decay-sawtooth.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  context-decay-sawtooth.png')


# --------------------------------------------------------------------------
# Figure 2: With injection (steady state)
# --------------------------------------------------------------------------

def fig_steady_state():
    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(12, 4.5), sharey=True)

    t = np.linspace(0.1, 30, 2000)
    user_turns = [5, 10, 15, 20, 25]
    way_injections = [7.5, 17.5, 27]

    alpha, beta = 0.38, 0.55

    # Left panel: without ways (same as fig 1 but smaller)
    adherence_no_ways = damped_sawtooth(t, user_turns, alpha=alpha, beta=beta)
    ax1.fill_between(t, adherence_no_ways, alpha=0.12, color=ORANGE)
    ax1.plot(t, adherence_no_ways, color=ORANGE, linewidth=2)
    ax1.axhline(0.15, color='#A0AEC0', linewidth=1, linestyle='--', alpha=0.6)
    ax1.set_title('Without Ways', pad=10, fontsize=12)
    ax1.set_xlabel('Conversation progression', fontsize=10)
    ax1.set_ylabel('Effective adherence', fontsize=11)
    ax1.set_xlim(0, 30)
    ax1.set_ylim(0, 1.15)
    ax1.set_xticks([])
    ax1.set_yticks([0, 0.25, 0.5, 0.75, 1.0])
    ax1.grid(True, axis='y')

    # Shade "lost instruction" zone
    ax1.fill_between(t, 0, 0.15, where=(adherence_no_ways < 0.15),
                     alpha=0.08, color='red')
    ax1.text(22, 0.06, 'instructions\nbelow noise floor', fontsize=8,
             color=ORANGE, ha='center', style='italic', alpha=0.8)

    # Right panel: with ways
    adherence_ways = injected_adherence(t, user_turns, way_injections,
                                        alpha=alpha, beta=beta,
                                        A_inject=0.65)
    ax2.fill_between(t, adherence_ways, alpha=0.12, color=GREEN)
    ax2.plot(t, adherence_ways, color=GREEN, linewidth=2,
             label='Combined adherence')

    # Show base system prompt decay faintly
    ax2.plot(t, adherence_no_ways, color=ORANGE, linewidth=1, alpha=0.3,
             linestyle='--', label='System prompt alone')

    # Mark way injections
    for wt in way_injections:
        ax2.axvline(wt, color=TEAL, linewidth=1.2, alpha=0.5, linestyle='-')
    ax2.plot([], [], color=TEAL, linewidth=1.2, alpha=0.5, label='Way injection')

    ax2.axhline(0.15, color='#A0AEC0', linewidth=1, linestyle='--', alpha=0.6)

    # Steady state annotation
    ax2.annotate('steady state', xy=(24, 0.65), fontsize=9, color=GREEN,
                 ha='center', style='italic',
                 bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                           edgecolor=GREEN, alpha=0.8))

    ax2.set_title('With Ways', pad=10, fontsize=12)
    ax2.set_xlabel('Conversation progression', fontsize=10)
    ax2.set_xlim(0, 30)
    ax2.set_xticks([])
    ax2.grid(True, axis='y')
    ax2.legend(loc='upper right', fontsize=8.5)

    fig.suptitle('Timed Injection Maintains Adherence Across Conversation Length',
                 fontsize=13, fontweight='bold', y=1.02)
    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'context-decay-comparison.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  context-decay-comparison.png')


# --------------------------------------------------------------------------
# Figure 3: Saturation curve
# --------------------------------------------------------------------------

def fig_saturation():
    fig, ax = plt.subplots(figsize=(8, 4.5))

    n_concurrent = np.linspace(0, 20, 200)
    A_inject = 0.85

    # Different competition coefficients
    k_values = [0.15, 0.3, 0.6]
    colors = [GREEN, TEAL, PURPLE]
    labels = ['Low competition (small ways)', 'Medium competition',
              'High competition (large ways)']

    for k, color, label in zip(k_values, colors, labels):
        A_eff = A_inject / (1 + k * n_concurrent)
        ax.plot(n_concurrent, A_eff, color=color, linewidth=2.2, label=label)

    # Mark the sweet spot
    ax.axvspan(1, 4, alpha=0.08, color=GREEN)
    ax.text(2.5, 0.88, 'sweet spot\n(1-4 ways)', fontsize=9, ha='center',
            color=GREEN, style='italic',
            bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                      edgecolor=GREEN, alpha=0.8))

    # Diminishing returns zone
    ax.axvspan(8, 20, alpha=0.05, color=ORANGE)
    ax.text(14, 0.55, 'diminishing\nreturns', fontsize=9, ha='center',
            color=ORANGE, style='italic', alpha=0.8)

    ax.set_xlabel('Concurrent active injections', fontsize=11)
    ax.set_ylabel('Effective adherence per injection', fontsize=11)
    ax.set_title('The Saturation Constraint: More Injections ≠ More Adherence',
                 pad=12)
    ax.set_xlim(0, 20)
    ax.set_ylim(0, 1.0)
    ax.set_xticks([0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20])
    ax.legend(loc='upper right', fontsize=9)
    ax.grid(True, alpha=0.5)

    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'context-decay-saturation.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  context-decay-saturation.png')


# --------------------------------------------------------------------------
# Figure 4: RoPE Positional Decay — frequency band decomposition
# --------------------------------------------------------------------------

def fig_rope_decay():
    fig, ax = plt.subplots(figsize=(10, 5))

    distance = np.arange(1, 513)
    d_model = 128
    base = 10000

    # Compute aggregate attention score (sum of all bands, normalized)
    aggregate = np.zeros_like(distance, dtype=float)
    for k in range(d_model // 2):
        theta_k = base ** (-2 * k / d_model)
        aggregate += np.cos(distance * theta_k)
    aggregate /= (d_model // 2)

    # Smooth envelope of aggregate for readability
    from scipy.ndimage import uniform_filter1d
    envelope = uniform_filter1d(np.abs(aggregate), size=30)

    # Show just two representative bands as faint background context
    # High-frequency: rapid oscillation, decays fast
    theta_high = base ** (-2 * 2 / d_model)
    band_high = np.cos(distance * theta_high)
    ax.plot(distance, band_high, color=ORANGE, linewidth=0.8, alpha=0.25)
    ax.text(80, 0.82, 'high-freq band\n(rapid decay)', fontsize=8.5,
            color=ORANGE, alpha=0.7, style='italic')

    # Low-frequency: slow oscillation, persists
    theta_low = base ** (-2 * 50 / d_model)
    band_low = np.cos(distance * theta_low)
    ax.plot(distance, band_low, color=PURPLE, linewidth=0.8, alpha=0.25)
    ax.text(350, 0.82, 'low-freq band\n(slow decay)', fontsize=8.5,
            color=PURPLE, alpha=0.7, style='italic')

    # Aggregate is the main story
    ax.fill_between(distance, aggregate, alpha=0.12, color=TEAL)
    ax.plot(distance, aggregate, color=TEAL, linewidth=2.2,
            label='Aggregate attention score', zorder=5)

    # Decay envelope
    ax.plot(distance, envelope, color='#1A1A2E', linewidth=1.5,
            linestyle='--', alpha=0.6, label='Decay envelope', zorder=6)

    # Mark the effective attention horizon
    ax.axvspan(0, 30, alpha=0.06, color=GREEN)
    ax.annotate('effective attention\nhorizon (~20-30 tokens)',
                xy=(30, 0.35), xytext=(100, 0.55),
                fontsize=9, color=GREEN, fontweight='bold',
                arrowprops=dict(arrowstyle='->', color=GREEN, lw=1.5),
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=GREEN, alpha=0.9))

    # System prompt position annotation
    ax.annotate('system prompt\nat distance n',
                xy=(300, 0.02), xytext=(350, 0.3),
                fontsize=9, color=ORANGE, fontweight='bold',
                arrowprops=dict(arrowstyle='->', color=ORANGE, lw=1.5),
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=ORANGE, alpha=0.9))

    ax.axhline(0, color='#A0AEC0', linewidth=0.5, alpha=0.5)
    ax.set_xlabel('Token distance |i − j| from generation cursor', fontsize=11)
    ax.set_ylabel('Attention score', fontsize=11)
    ax.set_title('RoPE Positional Decay: Why Distant Tokens Lose Influence',
                 pad=12)
    ax.set_xlim(0, 512)
    ax.set_ylim(-0.5, 1.05)
    ax.legend(loc='upper right', fontsize=9)
    ax.grid(True, alpha=0.4)

    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'formal-rope-decay.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  formal-rope-decay.png')


# --------------------------------------------------------------------------
# Figure 5: Operator Error Envelope — difficulty floor comparison
# --------------------------------------------------------------------------

def fig_error_envelope():
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(11, 7), sharex=True)

    np.random.seed(42)
    t = np.linspace(0, 30, 3000)

    # Strategic drift: slow, low-frequency error that grows over time
    strategic_drift = 0.08 * np.sin(0.3 * t) + 0.04 * t / 30

    # Tactical noise: high-frequency convention violations, format errors
    tactical_noise = (0.25 * np.sin(5.5 * t) +
                      0.15 * np.sin(13 * t + 1.2) +
                      0.1 * np.random.randn(len(t)))
    # Add occasional spikes (missed conventions)
    spikes = np.zeros_like(t)
    spike_positions = [3.5, 7.2, 11.8, 14.1, 18.5, 22.3, 26.7]
    for sp in spike_positions:
        spikes += 0.35 * np.exp(-8 * (t - sp)**2)

    tactical_noise += spikes

    # --- Top panel: WITHOUT WAYS ---
    error_no_ways = strategic_drift + tactical_noise
    ax1.fill_between(t, error_no_ways, alpha=0.15, color=ORANGE)
    ax1.plot(t, error_no_ways, color=ORANGE, linewidth=1.2,
             label='Total error (human must correct)')

    # Show the strategic component faintly
    ax1.plot(t, strategic_drift, color=PURPLE, linewidth=1.5, linestyle='--',
             alpha=0.6, label='Strategic drift only')

    # Difficulty threshold
    ax1.axhline(0.3, color='#E53E3E', linewidth=1.2, linestyle='-', alpha=0.6)
    ax1.text(29.5, 0.33, 'correction threshold', ha='right', fontsize=8.5,
             color='#E53E3E', style='italic')

    # Annotate high-freq content
    ax1.annotate('tactical spikes\n(convention violations,\nformat errors,\nmissed steps)',
                 xy=(7.2, 0.55), xytext=(4, 0.75),
                 fontsize=8.5, color=ORANGE,
                 arrowprops=dict(arrowstyle='->', color=ORANGE, lw=1.2),
                 bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                           edgecolor=ORANGE, alpha=0.9))

    # Required gain annotation
    ax1.annotate('', xy=(28, -0.15), xytext=(28, 0.65),
                 arrowprops=dict(arrowstyle='<->', color=SLATE, lw=1.5))
    ax1.text(27.5, 0.25, 'required\noperator\ngain K',
             fontsize=8.5, color=SLATE, ha='right', fontweight='bold')

    ax1.set_ylabel('Error magnitude\n(desired − actual)', fontsize=10)
    ax1.set_title('Without Ways — Operator Sees Full Error Spectrum',
                  pad=10, fontsize=12)
    ax1.set_ylim(-0.3, 0.9)
    ax1.legend(loc='upper left', fontsize=8.5)
    ax1.grid(True, axis='y', alpha=0.4)

    # --- Bottom panel: WITH WAYS ---
    # Inner loop absorbs tactical noise — only strategic drift remains
    # Plus some residual noise (inner loop isn't perfect)
    residual = 0.04 * np.random.randn(len(t))
    error_with_ways = strategic_drift + residual

    ax2.fill_between(t, error_with_ways, alpha=0.15, color=GREEN)
    ax2.plot(t, error_with_ways, color=GREEN, linewidth=1.2,
             label='Residual error (after inner loop)')

    # Show what was absorbed
    ax2.fill_between(t, error_with_ways, error_no_ways, alpha=0.06,
                     color=TEAL, label='Absorbed by ways (inner loop)')

    ax2.plot(t, strategic_drift, color=PURPLE, linewidth=1.5, linestyle='--',
             alpha=0.6, label='Strategic drift')

    # Same threshold
    ax2.axhline(0.3, color='#E53E3E', linewidth=1.2, linestyle='-', alpha=0.6)

    # Smaller required gain
    ax2.annotate('', xy=(28, -0.05), xytext=(28, 0.2),
                 arrowprops=dict(arrowstyle='<->', color=SLATE, lw=1.5))
    ax2.text(27.5, 0.07, 'reduced\ngain K',
             fontsize=8.5, color=SLATE, ha='right', fontweight='bold')

    # Difficulty floor annotation
    ax2.annotate('difficulty floor lowered —\nless engaged operator\nstill gets acceptable results',
                 xy=(18, 0.12), xytext=(12, 0.45),
                 fontsize=9, color=GREEN, fontweight='bold',
                 arrowprops=dict(arrowstyle='->', color=GREEN, lw=1.5),
                 bbox=dict(boxstyle='round,pad=0.4', facecolor='white',
                           edgecolor=GREEN, alpha=0.9))

    ax2.set_xlabel('Conversation progression', fontsize=10)
    ax2.set_ylabel('Error magnitude\n(desired − actual)', fontsize=10)
    ax2.set_title('With Ways — Inner Loop Absorbs Tactical Errors',
                  pad=10, fontsize=12)
    ax2.set_ylim(-0.3, 0.9)
    ax2.set_xticks([])
    ax2.legend(loc='upper left', fontsize=8.5)
    ax2.grid(True, axis='y', alpha=0.4)

    fig.suptitle('The Difficulty Floor: How Cascade Control Reduces Operator Workload',
                 fontsize=13, fontweight='bold', y=1.01)
    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'formal-error-envelope.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  formal-error-envelope.png')


# --------------------------------------------------------------------------
# Figure 6: Cascade Disturbance Rejection — Bode magnitude plot
# --------------------------------------------------------------------------

def fig_cascade_bode():
    fig, ax = plt.subplots(figsize=(10, 5.5))

    # Frequency axis (log scale): low freq = strategic, high freq = tactical
    freq = np.logspace(-2, 2, 500)

    # Disturbance spectrum: combination of low-freq strategic and high-freq tactical
    disturbance_spectrum = 0.5 / (1 + (freq / 0.3)**2) + 0.8 / (1 + ((freq - 8) / 3)**2)
    disturbance_spectrum += 0.05  # noise floor

    # Inner loop (ways) rejection: high-pass filter — attenuates high frequencies
    # Ways act as a disturbance rejection that handles tactical (high-freq) errors
    inner_loop_rejection = 1.0 / (1 + (freq / 1.5)**2)

    # What passes through to the human
    human_sees = disturbance_spectrum * inner_loop_rejection

    # Plot
    ax.fill_between(freq, disturbance_spectrum, alpha=0.12, color=ORANGE)
    ax.plot(freq, disturbance_spectrum, color=ORANGE, linewidth=2,
            label='Total disturbance spectrum')

    ax.fill_between(freq, human_sees, alpha=0.15, color=GREEN)
    ax.plot(freq, human_sees, color=GREEN, linewidth=2.2,
            label='What operator sees (after inner loop)')

    # Shade the absorbed region
    ax.fill_between(freq, human_sees, disturbance_spectrum,
                    where=(disturbance_spectrum > human_sees),
                    alpha=0.08, color=TEAL)

    # Mark the crossover frequency
    crossover_freq = 1.5
    ax.axvline(crossover_freq, color=SLATE, linewidth=1.5, linestyle='--',
               alpha=0.7)
    ax.text(crossover_freq * 1.15, 0.42, 'crossover\nfrequency',
            fontsize=9, color=SLATE, style='italic', fontweight='bold')

    # Label the regions
    ax.text(0.06, 0.35, 'Strategic errors\n(human handles)',
            fontsize=10, color=PURPLE, fontweight='bold',
            bbox=dict(boxstyle='round,pad=0.4', facecolor='white',
                      edgecolor=PURPLE, alpha=0.9))
    ax.text(12, 0.25, 'Tactical errors\n(ways handle)',
            fontsize=10, color=TEAL, fontweight='bold',
            bbox=dict(boxstyle='round,pad=0.4', facecolor='white',
                      edgecolor=TEAL, alpha=0.9))

    # Inner loop attenuation annotation
    ax.annotate('inner loop\nattenuation', xy=(6, 0.15), xytext=(20, 0.35),
                fontsize=9, color=TEAL,
                arrowprops=dict(arrowstyle='->', color=TEAL, lw=1.5),
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=TEAL, alpha=0.8))

    ax.set_xscale('log')
    ax.set_xlabel('Disturbance frequency\n(low = strategic drift, high = tactical errors)',
                  fontsize=10)
    ax.set_ylabel('Disturbance magnitude', fontsize=11)
    ax.set_title('Cascade Disturbance Rejection: What Each Loop Handles', pad=12)
    ax.set_xlim(0.03, 80)
    ax.set_ylim(0, 0.65)
    ax.legend(loc='upper right', fontsize=9)
    ax.grid(True, alpha=0.4, which='both')
    ax.set_xticks([0.1, 1, 10])
    ax.set_xticklabels(['0.1\n(slow drift)', '1\n(per-turn)', '10\n(per-tool-call)'])

    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'formal-cascade-bode.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  formal-cascade-bode.png')


# --------------------------------------------------------------------------
# Figure 7: Composite Adherence — stacked contributions
# --------------------------------------------------------------------------

def fig_composite_adherence():
    fig, ax = plt.subplots(figsize=(11, 5.5))

    t = np.linspace(0.1, 30, 2000)
    user_turns = [5, 10, 15, 20, 25]
    way_injections = [3, 7, 12, 17, 22, 27]

    alpha, beta = 0.38, 0.55

    # Component 1: System prompt (decaying)
    sys_prompt = damped_sawtooth(t, user_turns, alpha=alpha, beta=beta, A0=0.8)

    # Component 2: Ways (inner loop injections)
    ways_component = np.zeros_like(t)
    for inj_t in way_injections:
        for i, ti in enumerate(t):
            if ti >= inj_t:
                t_since = ti - inj_t
                contrib = 0.45 * np.exp(-beta * t_since)
                ways_component[i] = max(ways_component[i], contrib)

    # Component 3: Human steering (periodic corrections at user turns)
    human_component = np.zeros_like(t)
    for ut in user_turns:
        for i, ti in enumerate(t):
            if ti >= ut:
                t_since = ti - ut
                # Human correction is a sharp boost that decays
                contrib = 0.25 * np.exp(-0.8 * t_since)
                human_component[i] = max(human_component[i], contrib)

    # Stack them
    ax.fill_between(t, 0, sys_prompt, alpha=0.3, color=ORANGE,
                    label='System prompt (decaying baseline)')
    ax.fill_between(t, sys_prompt, sys_prompt + ways_component,
                    alpha=0.3, color=TEAL,
                    label='Ways — inner loop (timed injections)')
    ax.fill_between(t, sys_prompt + ways_component,
                    sys_prompt + ways_component + human_component,
                    alpha=0.3, color=PURPLE,
                    label='Human steering — outer loop (corrections)')

    # Total line
    total = sys_prompt + ways_component + human_component
    ax.plot(t, total, color='#1A1A2E', linewidth=2, label='Total adherence')

    # Threshold
    ax.axhline(0.3, color='#E53E3E', linewidth=1, linestyle='--', alpha=0.6)
    ax.text(29.5, 0.32, 'min threshold', ha='right', fontsize=8.5,
            color='#E53E3E', style='italic')

    # Annotate the dominance shift
    ax.annotate('system prompt\ndominates early',
                xy=(2, 0.7), fontsize=8.5, color=ORANGE,
                ha='center', style='italic',
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=ORANGE, alpha=0.8))
    ax.annotate('ways sustain\nadherence',
                xy=(20, 0.55), fontsize=8.5, color=TEAL,
                ha='center', style='italic',
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=TEAL, alpha=0.8))
    ax.annotate('human corrects\nstrategic drift',
                xy=(15.5, 0.82), fontsize=8.5, color=PURPLE,
                ha='center', style='italic',
                bbox=dict(boxstyle='round,pad=0.3', facecolor='white',
                          edgecolor=PURPLE, alpha=0.8))

    # Mark way injections
    for wt in way_injections:
        ax.axvline(wt, color=TEAL, linewidth=0.8, alpha=0.3, linestyle=':')

    # Mark user turns
    for ut in user_turns:
        ax.axvline(ut, color=PURPLE, linewidth=0.8, alpha=0.3, linestyle=':')

    ax.set_xlabel('Conversation progression (tokens / turns)', fontsize=10)
    ax.set_ylabel('Effective adherence', fontsize=11)
    ax.set_title('Composite Adherence: Three-Term Decomposition', pad=12)
    ax.set_xlim(0, 30)
    ax.set_ylim(0, 1.2)
    ax.set_xticks([])
    ax.set_yticks([0, 0.25, 0.5, 0.75, 1.0])
    ax.legend(loc='upper right', fontsize=8.5)
    ax.grid(True, axis='y', alpha=0.4)

    fig.tight_layout()
    fig.savefig(OUTPUT_DIR / 'formal-composite-adherence.png', dpi=180,
                bbox_inches='tight', facecolor=fig.get_facecolor())
    plt.close(fig)
    print('  formal-composite-adherence.png')


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------

if __name__ == '__main__':
    import sys

    all_figures = {
        'sawtooth': fig_damped_sawtooth,
        'steady': fig_steady_state,
        'saturation': fig_saturation,
        'rope': fig_rope_decay,
        'error': fig_error_envelope,
        'bode': fig_cascade_bode,
        'composite': fig_composite_adherence,
    }

    # Allow selective generation: python generate-decay-diagrams.py rope error bode composite
    requested = sys.argv[1:] if len(sys.argv) > 1 else list(all_figures.keys())

    print('Generating context decay diagrams...')
    for name in requested:
        if name in all_figures:
            all_figures[name]()
        else:
            print(f'  Unknown figure: {name} (available: {", ".join(all_figures.keys())})')
    print('Done.')
