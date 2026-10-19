<script lang="ts">
  // Stroke icons on a 24x24 grid (drawn for this app, in the style of common line-icon sets).
  type Name =
    | 'folder' | 'folder-open' | 'file' | 'package' | 'image' | 'audio' | 'film' | 'cube' | 'text' | 'table'
    | 'search' | 'settings' | 'chevron' | 'x' | 'check' | 'download' | 'play' | 'pause' | 'sun' | 'moon'
    | 'plug' | 'refresh' | 'trash' | 'list' | 'layers' | 'basket' | 'jobs' | 'arrow-right' | 'alert'
    | 'steam' | 'hdd' | 'home' | 'up' | 'link' | 'copy' | 'zoom-in' | 'zoom-out' | 'fit' | 'tag' | 'bolt' | 'info';

  let { name, size = 16 }: { name: Name | (string & {}); size?: number } = $props();

  const paths: Record<Name, string> = {
    folder: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',
    'folder-open': 'M3 18V7a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v2M3 18l2.5-7h16.5l-2.5 7a2 2 0 0 1-1.9 1.4H5a2 2 0 0 1-2-1.4z',
    file: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5',
    package: 'M12 3l8 4.5v9L12 21l-8-4.5v-9zM4 7.5l8 4.5 8-4.5M12 12v9',
    image: 'M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8.5 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM21 16l-5-5-8 8',
    audio: 'M4 9v6h4l5 4V5L8 9zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12',
    film: 'M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM7 5v14M17 5v14M3 9h4M3 15h4M17 9h4M17 15h4',
    cube: 'M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12v9M4 7.5l8 4.5 8-4.5',
    text: 'M5 6h14M5 10h14M5 14h9M5 18h12',
    table: 'M4 5h16v14H4zM4 10h16M4 15h16M10 5v14',
    search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4',
    settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
    chevron: 'M9 6l6 6-6 6',
    x: 'M6 6l12 12M18 6L6 18',
    check: 'M5 12.5l4.5 4.5L19 7.5',
    download: 'M12 4v12M7 11l5 5 5-5M5 20h14',
    play: 'M7 4.5v15l12-7.5z',
    pause: 'M8 5v14M16 5v14',
    sun: 'M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
    moon: 'M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z',
    plug: 'M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4',
    refresh: 'M20 11a8 8 0 0 0-14.5-3.5M4 4v4h4M4 13a8 8 0 0 0 14.5 3.5M20 20v-4h-4',
    trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3',
    list: 'M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01',
    layers: 'M12 3l9 5-9 5-9-5zM3 13l9 5 9-5M3 17.5l9 5 9-5',
    basket: 'M3 9h18l-2 10a2 2 0 0 1-2 1.6H7A2 2 0 0 1 5 19zM8 9l3-5M16 9l-3-5',
    jobs: 'M12 3v4M12 17v4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M3 12h4M17 12h4M5.6 18.4l2.8-2.8M15.6 8.4l2.8-2.8',
    'arrow-right': 'M5 12h14M13 6l6 6-6 6',
    alert: 'M12 4l9.5 16h-19zM12 10v4M12 17.5h.01',
    steam: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 13.5l-4-1.7M8.5 15.5l2.2.9M14.5 10.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM10.7 16.4a1.8 1.8 0 1 0-1.4-3.3',
    hdd: 'M4 14l2.5-8h11L20 14M4 14v4a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-4zM4 14h16M16.5 16.5h.01',
    home: 'M4 11l8-7 8 7M6 10v10h12V10M10 20v-6h4v6',
    up: 'M12 19V6M6 11l6-6 6 6',
    link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
    copy: 'M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM5 15V5a1 1 0 0 1 1-1h10',
    'zoom-in': 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4M11 8v6M8 11h6',
    'zoom-out': 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4M8 11h6',
    fit: 'M4 9V5h4M20 9V5h-4M4 15v4h4M20 15v4h-4',
    tag: 'M3 12V4h8l10 10-8 8zM7.5 8.5h.01',
    bolt: 'M13 3L5 14h6l-1 7 8-11h-6z',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7.5h.01',
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  class="icon"
>
  <path d={paths[name as Name] ?? paths.file} />
</svg>

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
