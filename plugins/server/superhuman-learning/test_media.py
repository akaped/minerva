import unittest
from unittest.mock import patch
from media import canonical_url, normalize_json3, language, MediaError, fetch_captions, youtube

class MediaTests(unittest.TestCase):
    def test_urls(self):
        for raw in ['https://youtu.be/abcdefghijk?t=4','https://www.youtube.com/watch?v=abcdefghijk&list=ignored','https://youtube.com/shorts/abcdefghijk']:
            self.assertEqual(canonical_url(raw),('abcdefghijk','https://www.youtube.com/watch?v=abcdefghijk'))
        for raw in ['http://youtu.be/abcdefghijk','https://youtube.com.evil.test/watch?v=abcdefghijk','https://localhost/watch?v=abcdefghijk','https://user@youtube.com/watch?v=abcdefghijk','file:///etc/passwd','https://youtube.com/playlist?list=x','https://youtu.be/a']:
            with self.assertRaises(MediaError): canonical_url(raw)
    def test_language_cannot_be_flags(self):
        for raw in ['--exec','en,all','../secret','en.*','']:
            with self.assertRaises(MediaError): language(raw)
    def test_normalization_preserves_timing_and_repetition(self):
        data={'events':[{'tStartMs':1000,'dDurationMs':2000,'segs':[{'utf8':'One &amp; two'}]}, {'tStartMs':2000,'dDurationMs':2000,'segs':[{'utf8':'One &amp; two'}]}, {'tStartMs':8000,'dDurationMs':1000,'segs':[{'utf8':'One &amp; two'}]}]}
        result=normalize_json3(data,10)
        self.assertEqual(len(result),2)
        self.assertEqual(result[0],{'id':'s1','text':'One & two','start':1,'end':4})
        self.assertEqual(result[1]['start'],8)
    def test_caption_ssrf(self):
        for raw in ['http://www.youtube.com/a','https://127.0.0.1/a','https://evil.example/a']:
            with self.assertRaises(MediaError): fetch_captions(raw)
        with patch('socket.getaddrinfo',return_value=[(2,1,6,'',('127.0.0.1',443))]):
            with self.assertRaises(MediaError): fetch_captions('https://www.youtube.com/api/timedtext')
    def test_subtitles_before_automatic_and_no_audio(self):
        import types,sys
        class Client:
            def __init__(self,options): self.options=options
            def __enter__(self): return self
            def __exit__(self,*args): pass
            def extract_info(self,url,download=False):
                return {'id':'abcdefghijk','title':'Lecture','duration':30,'subtitles':{'en':[{'ext':'json3','url':'human'}]},'automatic_captions':{'en':[{'ext':'json3','url':'auto'}]}}
        with patch.dict(sys.modules,{'yt_dlp':types.SimpleNamespace(YoutubeDL=Client)}), patch('media.fetch_captions',return_value={'events':[{'tStartMs':5000,'dDurationMs':3000,'segs':[{'utf8':'Learning'}]}]}) as fetch:
            result=youtube({'url':'https://youtu.be/abcdefghijk','language':'en'})
            fetch.assert_called_once_with('human')
            self.assertEqual(result['source']['captions'],'human')
            self.assertEqual(result['sources'][0]['start'],5)
    def test_no_captions_offers_fallback(self):
        import types,sys
        class Client:
            def __init__(self,_): pass
            def __enter__(self): return self
            def __exit__(self,*args): pass
            def extract_info(self,*args,**kwargs): return {'id':'abcdefghijk','duration':30}
        with patch.dict(sys.modules,{'yt_dlp':types.SimpleNamespace(YoutubeDL=Client)}):
            self.assertTrue(youtube({'url':'https://youtu.be/abcdefghijk'})['needs_audio'])
            with self.assertRaises(MediaError): youtube({'url':'https://youtu.be/abcdefghijk','audio':True,'authorized':True,'directory':'/tmp'})

if __name__=='__main__': unittest.main()
