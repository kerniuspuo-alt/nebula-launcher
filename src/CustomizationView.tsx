import { useEffect } from "react";

export interface CustomizationState {
  backgroundImage: string;
  fitMode: string;
  borderRadius: number;
  uiScale: number;
}


export const CustomizationView = () => {
  
  useEffect(() => {
    console.log("CustomizationView loaded");
  }, []);
  return null;
};
