export function relativeLuminance(hex) {
  const channels=[1,3,5].map(index=>parseInt(hex.slice(index,index+2),16)/255).map(value=>value<=.04045?value/12.92:((value+.055)/1.055)**2.4);
  return channels[0]*.2126+channels[1]*.7152+channels[2]*.0722;
}

export function contrastingInk(hex) {
  const luminance=relativeLuminance(hex);
  const darkContrast=(luminance+.05)/(relativeLuminance("#101210")+.05),whiteContrast=1.05/(luminance+.05);
  if(Math.max(darkContrast,whiteContrast)<4.5)return "#000000";
  return darkContrast>whiteContrast?"#101210":"#ffffff";
}
