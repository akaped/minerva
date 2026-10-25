import unittest
from worker import handle

class KnowledgeTests(unittest.TestCase):
    def request(self, item):
        import json
        return {"operation":"validate", "sources":[{"id":"b1","text":"Feedback improves learning."}], "answer":json.dumps({"items":[item]})}
    def item(self):
        return {"kind":"quote","text":"Feedback improves learning.","source_ids":["b1"],"grounding":"extraction"}
    def test_quote_preserved(self):
        self.assertEqual(handle(self.request(self.item()))["items"][0], self.item())
    def test_invented_citation_rejected(self):
        item=self.item(); item["source_ids"]=["missing"]
        with self.assertRaises(ValueError): handle(self.request(item))
    def test_invented_quote_rejected(self):
        item=self.item(); item["text"]="Invented quote"
        with self.assertRaises(ValueError): handle(self.request(item))
    def test_analysis_without_citation(self):
        item=self.item(); item.update(kind="question",grounding="analysis",source_ids=[])
        handle(self.request(item))
    def test_extraction_without_citation_rejected(self):
        item=self.item(); item["source_ids"]=[]
        with self.assertRaises(ValueError): handle(self.request(item))
    def test_bad_pattern_rejected(self):
        with self.assertRaises(ValueError): handle({"operation":"prepare","pattern":"../../secret","sources":[]})
    def test_prompts_load(self):
        for pattern in ["summarize","extract_wisdom"]:
            self.assertIn("untrusted",handle({"operation":"prepare","pattern":pattern,"sources":[]})["system"])

if __name__ == '__main__': unittest.main()
