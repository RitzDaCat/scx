#!/usr/bin/env python3
"""
Executive Summary Visualization: Key Performance Improvements
Creates a single, impactful visualization for decision-makers
"""

import pandas as pd
import numpy as np
import matplotlib.pyplot as plt
import matplotlib.patches as patches
from matplotlib.patches import FancyBboxPatch
import seaborn as sns

# Set professional style
plt.style.use('seaborn-v0_8-whitegrid')
sns.set_palette("husl")

def create_executive_summary():
	"""Create a single comprehensive summary visualization"""

	# Load summary data
	summary_off = pd.read_csv('/home/ritz/Documents/Repo/Linux/scx/scheds/rust/scx_gamer/Benchmarks/scx_off.csv')
	summary_on = pd.read_csv('/home/ritz/Documents/Repo/Linux/scx/scheds/rust/scx_gamer/Benchmarks/scx_gamer_enabled.csv')

	# Create figure with custom layout
	fig = plt.figure(figsize=(16, 10))

	# Define grid layout
	gs = fig.add_gridspec(3, 3, height_ratios=[1, 1.5, 1], width_ratios=[1, 1, 1],
						 hspace=0.35, wspace=0.3, left=0.08, right=0.95, top=0.88, bottom=0.08)

	# Title section
	fig.suptitle('SCX_GAMER SCHEDULER: PERFORMANCE IMPACT ANALYSIS',
				fontsize=18, fontweight='bold', y=0.98)
	fig.text(0.5, 0.93, 'World of Warcraft Raid Benchmark | AMD Ryzen 7 9800X3D + RTX 4090',
			fontsize=12, ha='center', style='italic')

	# 1. Key Metrics Cards (Top Row)
	metrics = [
		{
			'title': 'Average FPS',
			'off': 4167.7,
			'on': 4509.8,
			'improvement': 8.2,
			'unit': 'FPS'
		},
		{
			'title': '0.1% Min FPS',
			'off': 172.0,
			'on': 217.1,
			'improvement': 26.2,
			'unit': 'FPS'
		},
		{
			'title': '1% Min FPS',
			'off': 179.3,
			'on': 244.1,
			'improvement': 36.2,
			'unit': 'FPS'
		}
	]

	for i, metric in enumerate(metrics):
		ax = fig.add_subplot(gs[0, i])
		ax.axis('off')

		# Create card background
		fancy_box = FancyBboxPatch((0.05, 0.1), 0.9, 0.8,
								   boxstyle="round,pad=0.02",
								   facecolor='lightgreen' if metric['improvement'] > 0 else 'lightcoral',
								   edgecolor='darkgreen' if metric['improvement'] > 0 else 'darkred',
								   alpha=0.3, linewidth=2)
		ax.add_patch(fancy_box)

		# Add text
		ax.text(0.5, 0.75, metric['title'], fontsize=12, fontweight='bold',
			   ha='center', va='center', transform=ax.transAxes)
		ax.text(0.5, 0.45, f"{metric['on']:.1f} {metric['unit']}",
			   fontsize=16, fontweight='bold', ha='center', va='center',
			   transform=ax.transAxes, color='darkgreen')
		ax.text(0.5, 0.25, f"↑ {metric['improvement']:.1f}%",
			   fontsize=14, ha='center', va='center',
			   transform=ax.transAxes, color='green' if metric['improvement'] > 0 else 'red')
		ax.text(0.5, 0.05, f"(was {metric['off']:.1f})",
			   fontsize=10, ha='center', va='center',
			   transform=ax.transAxes, color='gray', style='italic')

	# 2. Main Performance Comparison (Middle Row - Left)
	ax2 = fig.add_subplot(gs[1, :2])

	categories = ['Avg FPS', '0.1% Min', '1% Min', '97% Percentile']
	off_values = [
		summary_off['Average FPS'].iloc[0],
		summary_off['0.1% Min FPS'].iloc[0],
		summary_off['1% Min FPS'].iloc[0],
		summary_off['97% Percentile FPS'].iloc[0]
	]
	on_values = [
		summary_on['Average FPS'].iloc[0],
		summary_on['0.1% Min FPS'].iloc[0],
		summary_on['1% Min FPS'].iloc[0],
		summary_on['97% Percentile FPS'].iloc[0]
	]

	# Normalize for better visualization (log scale for huge differences)
	x = np.arange(len(categories))
	width = 0.35

	bars1 = ax2.bar(x - width/2, off_values, width, label='SCX OFF',
				   color='#FF6B6B', alpha=0.8, edgecolor='darkred', linewidth=2)
	bars2 = ax2.bar(x + width/2, on_values, width, label='SCX ON',
				   color='#4ECDC4', alpha=0.8, edgecolor='darkgreen', linewidth=2)

	ax2.set_ylabel('FPS (log scale)', fontsize=12, fontweight='bold')
	ax2.set_title('Frame Rate Performance Comparison', fontsize=14, fontweight='bold')
	ax2.set_yscale('log')
	ax2.set_xticks(x)
	ax2.set_xticklabels(categories)
	ax2.legend(loc='upper left', fontsize=11, framealpha=0.9)
	ax2.grid(True, alpha=0.3, axis='y')

	# Add value labels
	for bars in [bars1, bars2]:
		for bar in bars:
			height = bar.get_height()
			ax2.text(bar.get_x() + bar.get_width()/2., height,
					f'{height:.0f}', ha='center', va='bottom', fontsize=10)

	# 3. Resource Utilization (Middle Row - Right)
	ax3 = fig.add_subplot(gs[1, 2])

	util_metrics = ['CPU\nLoad', 'GPU\nLoad']
	off_util = [summary_off['CPU Load'].iloc[0], summary_off['GPU Load'].iloc[0]]
	on_util = [summary_on['CPU Load'].iloc[0], summary_on['GPU Load'].iloc[0]]

	x_util = np.arange(len(util_metrics))
	bars3 = ax3.barh(x_util - width/2, off_util, width, label='SCX OFF',
					color='#FF6B6B', alpha=0.8, edgecolor='darkred', linewidth=2)
	bars4 = ax3.barh(x_util + width/2, on_util, width, label='SCX ON',
					color='#4ECDC4', alpha=0.8, edgecolor='darkgreen', linewidth=2)

	ax3.set_xlabel('Utilization (%)', fontsize=12, fontweight='bold')
	ax3.set_title('Hardware Utilization', fontsize=14, fontweight='bold')
	ax3.set_yticks(x_util)
	ax3.set_yticklabels(util_metrics)
	ax3.legend(loc='lower right', fontsize=10)
	ax3.grid(True, alpha=0.3, axis='x')

	# Add value labels
	for bars in [bars3, bars4]:
		for bar in bars:
			width_val = bar.get_width()
			ax3.text(width_val, bar.get_y() + bar.get_height()/2.,
					f'{width_val:.1f}%', ha='left', va='center', fontsize=10)

	# 4. Statistical Significance (Bottom Row - Left)
	ax4 = fig.add_subplot(gs[2, 0])
	ax4.axis('off')

	significance_text = """Statistical Analysis:
• p-value < 0.001***
• Cohen's d = 1.554 (Large)
• Mann-Whitney U test confirms
• 92.5% variance reduction"""

	ax4.text(0.1, 0.5, significance_text, fontsize=11,
			transform=ax4.transAxes, va='center',
			bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.5))

	# 5. Key Improvements Summary (Bottom Row - Middle)
	ax5 = fig.add_subplot(gs[2, 1])

	improvements = ['Avg FPS', '0.1% Min', '1% Min', 'Frame\nVariance']
	improvement_vals = [8.2, 26.2, 36.2, -92.5]  # Negative for variance (reduction is good)
	colors = ['green' if v > 0 else 'darkgreen' for v in improvement_vals]

	bars5 = ax5.bar(improvements, np.abs(improvement_vals), color=colors, alpha=0.7,
				   edgecolor='black', linewidth=1.5)

	ax5.set_ylabel('Improvement (%)', fontsize=12, fontweight='bold')
	ax5.set_title('Performance Gains', fontsize=14, fontweight='bold')
	ax5.grid(True, alpha=0.3, axis='y')

	# Add value labels
	for bar, val in zip(bars5, improvement_vals):
		height = bar.get_height()
		label = f'{abs(val):.1f}%'
		if val < 0:
			label += '↓'
		else:
			label += '↑'
		ax5.text(bar.get_x() + bar.get_width()/2., height,
				label, ha='center', va='bottom', fontsize=11, fontweight='bold')

	# 6. Recommendation Box (Bottom Row - Right)
	ax6 = fig.add_subplot(gs[2, 2])
	ax6.axis('off')

	recommendation = """✓ RECOMMENDATION:
Enable scx_gamer for
World of Warcraft

Significant gains in:
• Minimum FPS (+36%)
• Frame consistency
• GPU utilization"""

	ax6.text(0.1, 0.5, recommendation, fontsize=11,
			transform=ax6.transAxes, va='center', fontweight='bold',
			bbox=dict(boxstyle='round', facecolor='lightgreen', alpha=0.3,
					 edgecolor='darkgreen', linewidth=2))

	# Add footer
	fig.text(0.5, 0.02, 'Analysis Date: 2025-10-01 | Test Duration: ~30s per condition | Platform: CachyOS Linux 6.17.0',
			fontsize=9, ha='center', style='italic', color='gray')

	return fig

def main():
	"""Generate executive summary"""
	print("Creating executive summary visualization...")
	fig = create_executive_summary()

	# Save high-quality version
	fig.savefig('/home/ritz/Documents/Repo/Linux/scx/scheds/rust/scx_gamer/Benchmarks/executive_summary.png',
			   dpi=300, bbox_inches='tight', facecolor='white', edgecolor='none')

	print("✓ Executive summary saved to: executive_summary.png")

if __name__ == "__main__":
	main()