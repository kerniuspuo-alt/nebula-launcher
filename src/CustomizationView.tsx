import { useCallback, useEffect, useState } from "react";

export interface CustomizationState {
  backgroundImage: string;
  fitMode: string;
  borderRadius: number;
  uiScale: number;
}

const DEFAULT_STATE: CustomizationState = {
  backgroundImage: "",
  fitMode: "Cover",
  borderRadius: 12,
  uiScale: 1.0,
};

export const CustomizationView = () => {
  const [state, setState] = useState<CustomizationState>(DEFAULT_STATE);
  useEffect(() => {
    console.log("CustomizationView loaded");
  }, []);
  return null;
};
