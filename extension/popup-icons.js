// Local vector icons, shared with the packaged popup by make-icons.mjs.
export const uiIcon = (name='file') => {
 const paths={
  grid:'M3 3h6v6H3z M15 3h6v6h-6z M3 15h6v6H3z M15 15h6v6h-6z',
  download:'M12 3v12m-5-5 5 5 5-5M4 17v4h16v-4',
  clock:'M12 8v5l3 2 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  pause:'M8 5v14M16 5v14',play:'m8 4 12 8-12 8z',
  check:'m5 12 4 4L19 6',error:'M12 7v6m0 4v.1 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',
  folder:'M3 6h7l2 3h9v11H3z',settings:'M5 7h14M5 17h14M9 4v6M15 14v6',
  links:'m9 15 6-6 M8 16l-1 1a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0 M16 8l1-1a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0',
  file:'M5 3h9l5 5v13H5z M14 3v6h5',document:'M5 3h9l5 5v13H5z M14 3v6h5 M8 13h8M8 17h6',
  archive:'M4 4h16v17H4z M3 4V1h18v3 M10 8h4M10 12h4M10 16h4',
  music:'M10 17V5l10-2v12 M10 17a3 3 0 1 1-3-3h3 M20 15a3 3 0 1 1-3-3h3',
  video:'M3 5h18v14H3z m7 4 5 3-5 3z',image:'M3 3h18v18H3z m0 14 6-6 4 4 3-3 5 5 M15 7h.01',
  program:'M3 4h18v16H3z M3 9h18 m-11 3-3 2 3 2m4-4 3 2-3 2',
  plus:'M12 4v16M4 12h16',trash:'M3 6h18M9 6V3h6v3 M5 6l1 15h12l1-15 M10 10v7m4-7v7',
  close:'m7 7 10 10M17 7 7 17',up:'m6 14 6-6 6 6',down:'m6 10 6 6 6-6',
  save:'M4 3h13l3 3v15H4z M8 3v6h8V3 M8 21v-8h8v8',external:'M14 3h7v7 M21 3l-11 11 M10 3H3v18h18v-7',
  refresh:'M20 10a8 8 0 1 0 0 5 M20 4v6h-6',copy:'M9 9h12v12H9z M5 15H3V3h12v2',search:'M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14 m5 12 6 6'
 };
 return `<svg data-icon="${name}" class="ui-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="${paths[name]||paths.file}"/></svg>`;
};
