export type DjPlan = {
  mood: string;
  query: string;
  energy: number;
};

export function interpretDj(prompt: string): DjPlan {
  const p = prompt.toLowerCase();
  if (p.includes("phonk") || p.includes("pvp") || p.includes("clutch")) {
    return { mood: "High-energy PvP", query: "aggressive phonk drift phonk workout", energy: 0.95 };
  }
  if (p.includes("lofi") || p.includes("build") || p.includes("chill")) {
    return { mood: "Chill building", query: "lofi beats to build to", energy: 0.35 };
  }
  if (p.includes("boss") || p.includes("epic") || p.includes("nether")) {
    return { mood: "Epic raid", query: "epic orchestral trailer hybrid nether", energy: 0.8 };
  }
  if (p.includes("night") || p.includes("rain") || p.includes("cozy")) {
    return { mood: "Night rain", query: "cozy rainy night jazz lofi", energy: 0.25 };
  }
  if (p.includes("speedrun") || p.includes("fast")) {
    return { mood: "Speedrun", query: "drum and bass liquid dnb fast", energy: 0.88 };
  }
  return { mood: "Custom", query: prompt, energy: 0.5 };
}
