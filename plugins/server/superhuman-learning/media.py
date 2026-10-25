"""Optional media ingestion; no credentials, shells, or automatic dependency installs."""
import html
import http.client
import importlib.util
import ipaddress
import json
import math
import re
import socket
import ssl
from pathlib import Path
from urllib.parse import urlparse, parse_qs, urljoin

class MediaError(ValueError):
    pass

def configuration():
    path = Path(__file__).parent / 'config.json'
    config = {'preferred_language': 'en', 'max_duration_minutes': 120, 'allow_audio_fallback': False}
    if path.exists():
        value = json.loads(path.read_text())
        if not isinstance(value, dict) or set(value) - set(config):
            raise MediaError('Unknown plugin configuration field.')
        config.update(value)
    if not isinstance(config['allow_audio_fallback'], bool) or type(config['max_duration_minutes']) is not int or not 1 <= config['max_duration_minutes'] <= 240:
        raise MediaError('Invalid duration or audio fallback configuration.')
    language(config['preferred_language'])
    return config

def language(value):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8}){0,2}', value):
        raise MediaError('Use a language code such as en, it or pt-BR.')
    return value

def canonical_url(raw):
    try:
        u = urlparse(raw)
        if u.scheme != 'https' or u.username or u.password or u.port or any(c.isspace() for c in raw):
            raise ValueError()
        parts = u.path.strip('/').split('/')
        if u.hostname == 'youtu.be' and len(parts) == 1:
            video = parts[0]
        elif u.hostname in {'youtube.com', 'www.youtube.com', 'm.youtube.com'}:
            if u.path == '/watch':
                video = parse_qs(u.query).get('v', [''])[0]
            elif len(parts) == 2 and parts[0] in {'shorts', 'embed', 'live'}:
                video = parts[1]
            else:
                raise ValueError()
        else:
            raise ValueError()
        if not re.fullmatch(r'[A-Za-z0-9_-]{11}', video):
            raise ValueError()
        return video, 'https://www.youtube.com/watch?v=' + video
    except (ValueError, TypeError):
        raise MediaError('Enter a single HTTPS YouTube video URL (playlists and other sites are unsupported).')

def diagnostics():
    config = configuration()
    found = importlib.util.find_spec('yt_dlp') is not None
    return {'youtube': found, 'audio': True, 'allow_audio_fallback': config['allow_audio_fallback'],
            'preferred_language': config['preferred_language'],
            'diagnostics': [{'dependency': 'yt-dlp Python package', 'available': found,
                             'remediation': 'Install yt-dlp into the Python interpreter selected by MINERVA_PLUGIN_PYTHON.'}]}

def fetch_captions(raw):
    # Pin each HTTPS connection to a resolved public IP. Redirects repeat all checks.
    for _ in range(4):
        u = urlparse(raw)
        if u.scheme != 'https' or u.hostname not in {'www.youtube.com', 'youtube.com'} or u.port not in (None,443) or u.username or u.password:
            raise MediaError('YouTube returned an unsupported subtitle address.')
        addresses = socket.getaddrinfo(u.hostname, 443, type=socket.SOCK_STREAM)
        ips = [entry[4][0] for entry in addresses]
        if not ips or any(not ipaddress.ip_address(ip).is_global for ip in ips):
            raise MediaError('Subtitle address is not on the public internet.')
        connection = http.client.HTTPSConnection(u.hostname, timeout=20, context=ssl.create_default_context())
        connection._create_connection = lambda address, timeout, source_address=None: socket.create_connection((ips[0],443), timeout)
        try:
            connection.request('GET', u.path + ('?' + u.query if u.query else ''), headers={'User-Agent':'Mozilla/5.0'})
            response = connection.getresponse()
            if response.status in (301,302,303,307,308):
                raw = urljoin(raw,response.getheader('Location',''))
                continue
            if response.status != 200:
                raise MediaError('YouTube captions are unavailable or rate limited. Try again later.')
            data = response.read(2_000_001)
            if len(data) > 2_000_000:
                raise MediaError('Caption file exceeds the 2 MB limit.')
            return json.loads(data)
        finally:
            connection.close()
    raise MediaError('Too many subtitle redirects.')

def normalize_json3(data, duration):
    segments = []
    previous = None
    for event in data.get('events', []):
        text = html.unescape(''.join(s.get('utf8','') for s in event.get('segs',[]))).strip()
        if not text:
            continue
        start = float(event.get('tStartMs',0))/1000
        end = start + float(event.get('dDurationMs',0))/1000
        if not all(math.isfinite(v) for v in (start,end)) or start < 0 or end < start or start > duration+5:
            raise MediaError('Caption timestamps are invalid.')
        # Drop duplicate overlapping rolling-caption events, never nonoverlapping repetition.
        if previous and previous['text'] == text and start <= previous['end']:
            previous['end'] = max(previous['end'],min(end,duration))
            continue
        previous = {'id':f's{len(segments)+1}', 'start':start, 'end':min(max(start,end),duration), 'text':text}
        segments.append(previous)
    if not segments or len(segments)>6000 or sum(len(s['text'].encode()) for s in segments)>192000:
        raise MediaError('Transcript is empty or exceeds the 192 KB / 6,000 segment limit.')
    return segments

class Quiet:
    def debug(self, *_): pass
    def warning(self, *_): pass
    def error(self, *_): pass

def youtube(request):
    config = configuration()
    video, url = canonical_url(request['url'])
    preferred = language(request.get('language') or config['preferred_language'])
    try:
        import yt_dlp
    except ImportError:
        raise MediaError('YouTube ingestion needs the yt-dlp Python package. Ask the server administrator to install it.')
    options = {'quiet':True, 'no_warnings':True, 'logger':Quiet(), 'noplaylist':True,
               'socket_timeout':20, 'retries':1, 'extractor_retries':1, 'proxy':'',
               'skip_download':True, 'cachedir':False, 'js_runtimes':{}, 'remote_components':[],
               'allowed_extractors':['youtube'], 'geo_bypass':False}
    try:
        with yt_dlp.YoutubeDL(options) as client:
            info = client.extract_info(url, download=False)
    except Exception:
        raise MediaError('YouTube could not be read. It may be private, restricted, unavailable, or require a newer yt-dlp. No access bypass is attempted.')
    duration = info.get('duration')
    if info.get('id') != video or info.get('is_live') or not isinstance(duration,(int,float)) or not math.isfinite(duration) or not 0 < duration <= config['max_duration_minutes']*60:
        raise MediaError('Choose a finished video within the configured duration limit.')
    source = {'type':'youtube','id':video,'url':url,'title':str(info.get('title','YouTube video'))[:500],
              'author':str(info.get('channel') or info.get('uploader') or '')[:500], 'duration':duration, 'language':preferred}
    if request.get('audio'):
        if not config['allow_audio_fallback'] or not request.get('authorized'):
            raise MediaError('Audio fallback requires administrator enablement and confirmation that you may process the audio.')
        directory = Path(request['directory'])
        target = directory / 'source.m4a'
        def limit(progress):
            if progress.get('downloaded_bytes',0)>20*1024*1024:
                raise MediaError('Audio exceeds the 20 MiB limit.')
        options.update({'skip_download':False,'format':'bestaudio[ext=m4a][protocol=https]',
                        'outtmpl':str(target),'max_filesize':20*1024*1024,
                        'progress_hooks':[limit], 'nopart':True, 'overwrites':True})
        try:
            with yt_dlp.YoutubeDL(options) as client:
                client.download([url])
        except Exception:
            raise MediaError('Permitted audio retrieval failed or exceeded the size limit. Try an uploaded recording instead.')
        if not target.is_file() or not 0<target.stat().st_size<=20*1024*1024:
            raise MediaError('No supported audio within the size limit was available.')
        return {'source':source,'audio_file':'source.m4a'}
    # Prefer human captions, then automatic captions, without silently changing language.
    for kind in ('subtitles','automatic_captions'):
        tracks = info.get(kind) or {}
        candidates = [preferred] + sorted(k for k in tracks if k.startswith(preferred+'-'))
        for chosen in candidates:
            track = next((t for t in tracks.get(chosen,[]) if t.get('ext')=='json3'),None)
            if track:
                source.update(language=chosen, captions='automatic' if kind=='automatic_captions' else 'human')
                return {'source':source,'sources':normalize_json3(fetch_captions(track['url']),duration)}
    return {'source':source,'needs_audio':True,
            'error':'No captions in this language. Choose another language or enable permitted audio fallback.',
            'available_languages':sorted(set(info.get('subtitles',{})) | set(info.get('automatic_captions',{})))[:100]}
